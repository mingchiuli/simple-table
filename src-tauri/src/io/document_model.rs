use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::error::AppError;
use crate::formula::engine::{FormulaCellRef, FormulaRuntime};
use crate::io::codec::writer;
use crate::io::workbook_state;
use crate::ops::AppliedOperation;
use crate::state::content_hash::{ContentHash, hash_file_content};
use crate::types::FormulaStatus;
use crate::types::{AppliedOperationResult, CellValue, FileData, SheetCellChange, SheetData};
use umya_spreadsheet::{Workbook, Worksheet};

#[derive(Debug, Clone)]
pub struct DocumentOperationResult {
    pub operation: AppliedOperationResult,
    pub cell_changes: Vec<SheetCellChange>,
}

enum SpreadsheetDocumentBody {
    Excel(ExcelDocumentBody),
    Csv,
    GeneratedWorkbook,
}

struct ExcelDocumentBody {
    workbook: Box<Workbook>,
}

pub(crate) struct DocumentMemento {
    before: DocumentMementoSide,
    after: DocumentMementoSide,
}

pub(crate) enum DocumentMementoSide {
    Cells(CellMemento),
    Layout(LayoutMemento),
    Structure(StructureMemento),
}

pub(crate) struct CellMemento {
    cells: Vec<SheetCellChange>,
    sheet_shapes: Vec<SheetShapeMemento>,
}

struct SheetShapeMemento {
    sheet_index: usize,
    row_lengths: Vec<usize>,
}

pub(crate) struct LayoutMemento {
    sheet_index: usize,
    column_widths: HashMap<usize, Option<u32>>,
    row_heights: HashMap<usize, Option<u32>>,
}

pub(crate) struct StructureMemento {
    projection: FileStructureMemento,
    body: BodyStructureMemento,
}

pub(crate) struct FileStructureMemento {
    sheet_count: usize,
    sheets: Vec<SheetSnapshot>,
}

pub(crate) struct SheetSnapshot {
    sheet_index: usize,
    sheet: SheetData,
}

enum BodyStructureMemento {
    ExcelSheets {
        sheet_count: usize,
        sheets: Vec<WorksheetSnapshot>,
    },
    ExcelWorkbook {
        workbook: Box<Workbook>,
    },
    ProjectionOnly,
}

struct WorksheetSnapshot {
    sheet_index: usize,
    worksheet: Worksheet,
}

pub(crate) enum MementoSide {
    Before,
    After,
}

/// Canonical spreadsheet document.
///
/// Excel files keep the original `Workbook` as the persistence object. `FileData`
/// is a projection used by UI, formula calculation, search, and dirty hashing.
pub struct SpreadsheetDocument {
    projection: FileData,
    body: SpreadsheetDocumentBody,
    formula_runtime: FormulaRuntime,
    formula_status: FormulaStatus,
}

struct DocumentTransaction<'a> {
    document: &'a mut SpreadsheetDocument,
    operation: &'a AppliedOperation,
    rollback: &'a DocumentMementoSide,
}

impl<'a> DocumentTransaction<'a> {
    fn new(
        document: &'a mut SpreadsheetDocument,
        operation: &'a AppliedOperation,
        rollback: &'a DocumentMementoSide,
    ) -> Self {
        Self {
            document,
            operation,
            rollback,
        }
    }

    fn commit(&mut self) -> Result<DocumentOperationResult, AppError> {
        let result = self.operation.execute(&mut self.document.projection);

        if let Err(error) =
            self.document
                .patch_workbook_after_operation(self.operation, &result, &[])
        {
            self.rollback();
            return Err(error);
        }

        let cell_changes = self.document.recalculate_after_operation(self.operation);

        if !cell_changes.is_empty()
            && let Err(error) = self.document.patch_workbook_formula_changes(&cell_changes)
        {
            self.rollback();
            return Err(error);
        }

        Ok(DocumentOperationResult {
            operation: result,
            cell_changes,
        })
    }

    fn rollback(&mut self) {
        let _ = self.document.restore_memento_side(self.rollback);
    }
}

impl Clone for SpreadsheetDocument {
    fn clone(&self) -> Self {
        let mut projection = self.projection.clone();
        let formula_runtime =
            FormulaRuntime::new(&mut projection).unwrap_or_else(|_| FormulaRuntime::empty());
        Self {
            projection,
            body: self.clone_body(),
            formula_runtime,
            formula_status: self.formula_status.clone(),
        }
    }
}

impl SpreadsheetDocument {
    pub fn new(mut projection: FileData, workbook: Option<Workbook>) -> Self {
        let (formula_runtime, formula_status) = match FormulaRuntime::new(&mut projection) {
            Ok(runtime) => (runtime, FormulaStatus::Ready),
            Err(error) => {
                eprintln!("Formula runtime initialization failed: {error}");
                (
                    FormulaRuntime::empty(),
                    FormulaStatus::Degraded {
                        message: error.to_string(),
                    },
                )
            }
        };

        let body = match workbook {
            Some(workbook) => SpreadsheetDocumentBody::Excel(ExcelDocumentBody {
                workbook: Box::new(workbook),
            }),
            None if is_csv_document(&projection) => SpreadsheetDocumentBody::Csv,
            None => SpreadsheetDocumentBody::GeneratedWorkbook,
        };

        Self {
            projection,
            body,
            formula_runtime,
            formula_status,
        }
    }

    pub fn projection(&self) -> &FileData {
        &self.projection
    }

    pub fn content_hash(&self) -> ContentHash {
        hash_file_content(&self.projection)
    }

    pub fn formula_status(&self) -> FormulaStatus {
        self.formula_status.clone()
    }

    pub fn generate_file_bytes_for_target(
        &self,
        target_path_or_name: &str,
    ) -> Result<(String, Vec<u8>), AppError> {
        let extension = Path::new(target_path_or_name)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_lowercase())
            .unwrap_or_else(|| "xlsx".to_string());

        match extension.as_str() {
            "xlsx" => match &self.body {
                SpreadsheetDocumentBody::Excel(body) => {
                    writer::generate_excel_bytes_from_workbook_for_target(
                        &body.workbook,
                        target_path_or_name,
                    )
                }
                SpreadsheetDocumentBody::Csv | SpreadsheetDocumentBody::GeneratedWorkbook => {
                    writer::generate_file_bytes_for_target(&self.projection, target_path_or_name)
                }
            },
            "csv" => writer::generate_file_bytes_for_target(&self.projection, target_path_or_name),
            _ => Err(AppError::UnsupportedFormat),
        }
    }

    pub fn execute_operation(
        &mut self,
        operation: &AppliedOperation,
        rollback: &DocumentMementoSide,
    ) -> Result<DocumentOperationResult, AppError> {
        DocumentTransaction::new(self, operation, rollback).commit()
    }

    pub fn create_memento(
        before: DocumentMementoSide,
        after: DocumentMementoSide,
    ) -> DocumentMemento {
        DocumentMemento { before, after }
    }

    pub fn capture_memento_side(&self, operation: &AppliedOperation) -> DocumentMementoSide {
        match operation {
            AppliedOperation::SetCell {
                sheet_index,
                row,
                col,
                ..
            } => DocumentMementoSide::Cells(self.cell_memento([FormulaCellRef {
                sheet_index: *sheet_index,
                row: *row,
                col: *col,
            }])),
            AppliedOperation::SetCells { changes } => {
                DocumentMementoSide::Cells(self.cell_batch_memento(changes))
            }
            AppliedOperation::SetColumnWidth {
                sheet_index,
                col_index,
                ..
            } => DocumentMementoSide::Layout(self.layout_memento(
                *sheet_index,
                Some(*col_index),
                None,
            )),
            AppliedOperation::SetRowHeight {
                sheet_index,
                row_index,
                ..
            } => DocumentMementoSide::Layout(self.layout_memento(
                *sheet_index,
                None,
                Some(*row_index),
            )),
            AppliedOperation::AddRow { sheet_index, .. }
            | AppliedOperation::DeleteRow { sheet_index, .. }
            | AppliedOperation::AddColumn { sheet_index, .. }
            | AppliedOperation::DeleteColumn { sheet_index, .. } => {
                DocumentMementoSide::Structure(self.sheet_structure_memento([*sheet_index]))
            }
            AppliedOperation::AddSheet { sheet_index, .. }
            | AppliedOperation::DeleteSheet { sheet_index } => {
                let start = *sheet_index;
                DocumentMementoSide::Structure(
                    self.workbook_structure_memento(start..self.projection.sheets.len()),
                )
            }
        }
    }

    pub fn restore_memento(
        &mut self,
        memento: &DocumentMemento,
        side: MementoSide,
    ) -> Result<(), AppError> {
        match side {
            MementoSide::Before => self.restore_memento_side(&memento.before),
            MementoSide::After => self.restore_memento_side(&memento.after),
        }
    }

    fn restore_memento_side(&mut self, side: &DocumentMementoSide) -> Result<(), AppError> {
        match side {
            DocumentMementoSide::Cells(memento) => self.restore_cells(memento),
            DocumentMementoSide::Layout(memento) => self.restore_layout(memento),
            DocumentMementoSide::Structure(memento) => self.restore_structure(memento),
        }
    }

    fn cell_memento(&self, changed_cells: impl IntoIterator<Item = FormulaCellRef>) -> CellMemento {
        let changed_cells: Vec<FormulaCellRef> = changed_cells.into_iter().collect();
        let formula_cells = match &self.formula_status {
            FormulaStatus::Ready => self
                .formula_runtime
                .impacted_formula_cells_for(changed_cells.iter().copied()),
            FormulaStatus::Degraded { .. } => self.formula_cell_positions(),
        };
        self.cell_positions_memento(
            changed_cells
                .into_iter()
                .chain(formula_cells)
                .map(|cell| (cell.sheet_index, cell.row, cell.col)),
        )
    }

    fn cell_batch_memento(
        &self,
        changes: &[crate::ops::core_ops::ResolvedCellEdit],
    ) -> CellMemento {
        self.cell_memento(changes.iter().map(|change| FormulaCellRef {
            sheet_index: change.sheet_index,
            row: change.row,
            col: change.col,
        }))
    }

    fn cell_positions_memento(
        &self,
        positions_to_capture: impl IntoIterator<Item = (usize, usize, usize)>,
    ) -> CellMemento {
        let mut positions = Vec::new();
        let mut seen = HashSet::new();
        for (sheet_index, row, col) in positions_to_capture {
            push_unique_position(&mut positions, &mut seen, sheet_index, row, col);
        }
        let sheet_shapes = positions
            .iter()
            .map(|(sheet_index, _, _)| *sheet_index)
            .collect::<HashSet<_>>()
            .into_iter()
            .map(|sheet_index| SheetShapeMemento {
                sheet_index,
                row_lengths: self
                    .projection
                    .sheets
                    .get(sheet_index)
                    .map(|sheet| sheet.rows.iter().map(Vec::len).collect())
                    .unwrap_or_default(),
            })
            .collect();

        let cells = positions
            .into_iter()
            .map(|(sheet_index, row, col)| SheetCellChange {
                sheet_index,
                row,
                col,
                value: self.projection_cell(sheet_index, row, col),
            })
            .collect();

        CellMemento {
            cells,
            sheet_shapes,
        }
    }

    fn formula_cell_positions(&self) -> Vec<FormulaCellRef> {
        let mut positions = self.formula_runtime.all_formula_cells();
        let mut seen: HashSet<_> = positions.iter().copied().collect();
        for (sheet_index, sheet) in self.projection.sheets.iter().enumerate() {
            for (row, row_data) in sheet.rows.iter().enumerate() {
                for (col, cell) in row_data.iter().enumerate() {
                    if matches!(cell, CellValue::Formula { .. }) {
                        let cell_ref = FormulaCellRef {
                            sheet_index,
                            row,
                            col,
                        };
                        if seen.insert(cell_ref) {
                            positions.push(cell_ref);
                        }
                    }
                }
            }
        }
        positions
    }

    fn layout_memento(
        &self,
        sheet_index: usize,
        col_index: Option<usize>,
        row_index: Option<usize>,
    ) -> LayoutMemento {
        let mut column_widths = HashMap::new();
        let mut row_heights = HashMap::new();
        if let Some(col_index) = col_index {
            column_widths.insert(
                col_index,
                self.projection
                    .sheets
                    .get(sheet_index)
                    .and_then(|sheet| sheet.column_widths.as_ref())
                    .and_then(|widths| widths.get(&col_index).copied()),
            );
        }
        if let Some(row_index) = row_index {
            row_heights.insert(
                row_index,
                self.projection
                    .sheets
                    .get(sheet_index)
                    .and_then(|sheet| sheet.row_heights.as_ref())
                    .and_then(|heights| heights.get(&row_index).copied()),
            );
        }

        LayoutMemento {
            sheet_index,
            column_widths,
            row_heights,
        }
    }

    fn sheet_structure_memento(
        &self,
        sheet_indexes: impl IntoIterator<Item = usize>,
    ) -> StructureMemento {
        let sheet_indexes: Vec<usize> = sheet_indexes.into_iter().collect();
        StructureMemento {
            projection: self.projection_structure_memento(&sheet_indexes),
            body: match &self.body {
                SpreadsheetDocumentBody::Excel(body) => BodyStructureMemento::ExcelSheets {
                    sheet_count: body.workbook.sheet_count(),
                    sheets: sheet_indexes
                        .iter()
                        .filter_map(|sheet_index| {
                            body.workbook
                                .sheet(*sheet_index)
                                .ok()
                                .cloned()
                                .map(|worksheet| WorksheetSnapshot {
                                    sheet_index: *sheet_index,
                                    worksheet,
                                })
                        })
                        .collect(),
                },
                SpreadsheetDocumentBody::Csv | SpreadsheetDocumentBody::GeneratedWorkbook => {
                    BodyStructureMemento::ProjectionOnly
                }
            },
        }
    }

    fn workbook_structure_memento(
        &self,
        sheet_indexes: impl IntoIterator<Item = usize>,
    ) -> StructureMemento {
        let sheet_indexes: Vec<usize> = sheet_indexes.into_iter().collect();
        StructureMemento {
            projection: self.projection_structure_memento(&sheet_indexes),
            body: match &self.body {
                SpreadsheetDocumentBody::Excel(body) => BodyStructureMemento::ExcelWorkbook {
                    workbook: body.workbook.clone(),
                },
                SpreadsheetDocumentBody::Csv | SpreadsheetDocumentBody::GeneratedWorkbook => {
                    BodyStructureMemento::ProjectionOnly
                }
            },
        }
    }

    fn projection_structure_memento(&self, sheet_indexes: &[usize]) -> FileStructureMemento {
        FileStructureMemento {
            sheet_count: self.projection.sheets.len(),
            sheets: sheet_indexes
                .iter()
                .filter_map(|sheet_index| {
                    self.projection
                        .sheets
                        .get(*sheet_index)
                        .cloned()
                        .map(|sheet| SheetSnapshot {
                            sheet_index: *sheet_index,
                            sheet,
                        })
                })
                .collect(),
        }
    }

    fn projection_cell(&self, sheet_index: usize, row: usize, col: usize) -> CellValue {
        self.projection
            .sheets
            .get(sheet_index)
            .and_then(|sheet| sheet.rows.get(row))
            .and_then(|row_data| row_data.get(col))
            .cloned()
            .unwrap_or(CellValue::Null)
    }

    fn restore_cells(&mut self, memento: &CellMemento) -> Result<(), AppError> {
        for change in &memento.cells {
            let Some(sheet) = self.projection.sheets.get_mut(change.sheet_index) else {
                continue;
            };
            ensure_projection_cell_exists(sheet, change.row, change.col);
            sheet.rows[change.row][change.col] = change.value.clone();
        }

        self.patch_workbook_formula_changes(&memento.cells)?;
        self.restore_cell_shapes(&memento.sheet_shapes);
        self.patch_workbook_cell_shapes(&memento.sheet_shapes)?;
        self.rebuild_formula_runtime();
        Ok(())
    }

    fn restore_cell_shapes(&mut self, shapes: &[SheetShapeMemento]) {
        for shape in shapes {
            let Some(sheet) = self.projection.sheets.get_mut(shape.sheet_index) else {
                continue;
            };
            sheet.rows.truncate(shape.row_lengths.len());
            for (row, len) in shape.row_lengths.iter().copied().enumerate() {
                if let Some(row_data) = sheet.rows.get_mut(row) {
                    row_data.truncate(len);
                }
            }
        }
    }

    fn restore_layout(&mut self, memento: &LayoutMemento) -> Result<(), AppError> {
        let Some(sheet) = self.projection.sheets.get_mut(memento.sheet_index) else {
            return Ok(());
        };

        for (col_index, width) in &memento.column_widths {
            match width {
                Some(width) => {
                    sheet
                        .column_widths
                        .get_or_insert_with(Default::default)
                        .insert(*col_index, *width);
                }
                None => {
                    if let Some(widths) = sheet.column_widths.as_mut() {
                        widths.remove(col_index);
                        if widths.is_empty() {
                            sheet.column_widths = None;
                        }
                    }
                }
            }
        }

        for (row_index, height) in &memento.row_heights {
            match height {
                Some(height) => {
                    sheet
                        .row_heights
                        .get_or_insert_with(Default::default)
                        .insert(*row_index, *height);
                }
                None => {
                    if let Some(heights) = sheet.row_heights.as_mut() {
                        heights.remove(row_index);
                        if heights.is_empty() {
                            sheet.row_heights = None;
                        }
                    }
                }
            }
        }

        self.patch_workbook_layout(memento)
    }

    fn restore_structure(&mut self, memento: &StructureMemento) -> Result<(), AppError> {
        match &memento.body {
            BodyStructureMemento::ExcelSheets {
                sheet_count,
                sheets,
            } => {
                if let SpreadsheetDocumentBody::Excel(body) = &mut self.body {
                    restore_workbook_sheets(&mut body.workbook, *sheet_count, sheets)?;
                    self.refresh_projection_from_workbook();
                } else {
                    restore_projection_sheets(&mut self.projection, &memento.projection);
                }
            }
            BodyStructureMemento::ExcelWorkbook { workbook } => {
                self.body = SpreadsheetDocumentBody::Excel(ExcelDocumentBody {
                    workbook: workbook.clone(),
                });
                self.refresh_projection_from_workbook();
            }
            BodyStructureMemento::ProjectionOnly => {
                restore_projection_sheets(&mut self.projection, &memento.projection);
            }
        }
        self.rebuild_formula_runtime();
        Ok(())
    }

    fn recalculate_after_operation(
        &mut self,
        operation: &AppliedOperation,
    ) -> Vec<SheetCellChange> {
        match operation {
            AppliedOperation::SetCell {
                sheet_index,
                row,
                col,
                new_value,
                ..
            } => {
                let result = self.formula_runtime.sync_cell_and_recalculate(
                    &mut self.projection,
                    *sheet_index,
                    *row,
                    *col,
                );

                match result {
                    Ok(changes) => {
                        self.formula_status = FormulaStatus::Ready;
                        changes
                    }
                    Err(error) => {
                        eprintln!("Formula recalculation failed: {error}");
                        let changes = self.formula_error_change(
                            *sheet_index,
                            *row,
                            *col,
                            new_value,
                            error.to_string(),
                        );
                        self.rebuild_formula_runtime();
                        changes
                    }
                }
            }
            AppliedOperation::SetCells { changes } => {
                let changed_cell_refs: Vec<FormulaCellRef> = changes
                    .iter()
                    .map(|change| FormulaCellRef {
                        sheet_index: change.sheet_index,
                        row: change.row,
                        col: change.col,
                    })
                    .collect();
                match self
                    .formula_runtime
                    .sync_cells_and_recalculate(&mut self.projection, changed_cell_refs)
                {
                    Ok(changes) => {
                        self.formula_status = FormulaStatus::Ready;
                        changes
                    }
                    Err(error) => {
                        eprintln!("Formula recalculation failed: {error}");
                        let error = error.to_string();
                        let mut formula_errors = Vec::new();
                        for change in changes {
                            formula_errors.extend(self.formula_error_change(
                                change.sheet_index,
                                change.row,
                                change.col,
                                &change.new_value,
                                error.clone(),
                            ));
                        }
                        self.rebuild_formula_runtime();
                        formula_errors
                    }
                }
            }
            AppliedOperation::SetColumnWidth { .. } | AppliedOperation::SetRowHeight { .. } => {
                Vec::new()
            }
            _ => match self.formula_runtime.rebuild(&mut self.projection) {
                Ok(changes) => {
                    self.formula_status = FormulaStatus::Ready;
                    changes
                }
                Err(error) => {
                    eprintln!("Formula recalculation failed: {error}");
                    self.formula_error_changes_for_all_formulas(error.to_string())
                }
            },
        }
    }

    fn formula_error_change(
        &mut self,
        sheet_index: usize,
        row: usize,
        col: usize,
        value: &CellValue,
        error: String,
    ) -> Vec<SheetCellChange> {
        if !matches!(value, CellValue::Formula { .. }) {
            return Vec::new();
        }

        let Some(cell) = self
            .projection
            .sheets
            .get_mut(sheet_index)
            .and_then(|sheet| sheet.rows.get_mut(row))
            .and_then(|row_data| row_data.get_mut(col))
        else {
            return Vec::new();
        };

        *cell = cell.with_formula_result(CellValue::Null, Some(error));
        vec![SheetCellChange {
            sheet_index,
            row,
            col,
            value: cell.clone(),
        }]
    }

    fn formula_error_changes_for_all_formulas(&mut self, error: String) -> Vec<SheetCellChange> {
        let mut changes = Vec::new();
        for (sheet_index, sheet) in self.projection.sheets.iter_mut().enumerate() {
            for (row, row_data) in sheet.rows.iter_mut().enumerate() {
                for (col, cell) in row_data.iter_mut().enumerate() {
                    if !matches!(cell, CellValue::Formula { .. }) {
                        continue;
                    }
                    *cell = cell.with_formula_result(CellValue::Null, Some(error.clone()));
                    changes.push(SheetCellChange {
                        sheet_index,
                        row,
                        col,
                        value: cell.clone(),
                    });
                }
            }
        }
        self.formula_runtime = FormulaRuntime::empty();
        self.formula_status = FormulaStatus::Degraded { message: error };
        changes
    }

    fn rebuild_formula_runtime(&mut self) {
        match self.formula_runtime.rebuild(&mut self.projection) {
            Ok(_) => {
                self.formula_status = FormulaStatus::Ready;
            }
            Err(error) => {
                eprintln!("Formula runtime rebuild failed: {error}");
                self.formula_runtime = FormulaRuntime::empty();
                self.formula_status = FormulaStatus::Degraded {
                    message: error.to_string(),
                };
            }
        }
    }

    fn patch_workbook_after_operation(
        &mut self,
        operation: &AppliedOperation,
        result: &AppliedOperationResult,
        cell_changes: &[SheetCellChange],
    ) -> Result<(), AppError> {
        match &mut self.body {
            SpreadsheetDocumentBody::Excel(body) => workbook_state::patch_after_operation(
                &mut body.workbook,
                &mut self.projection,
                operation,
                result,
                cell_changes,
            )
            .map_err(|error| AppError::WorkbookPatchFailed(error.to_string())),
            SpreadsheetDocumentBody::Csv | SpreadsheetDocumentBody::GeneratedWorkbook => Ok(()),
        }
    }

    fn patch_workbook_formula_changes(
        &mut self,
        cell_changes: &[SheetCellChange],
    ) -> Result<(), AppError> {
        match &mut self.body {
            SpreadsheetDocumentBody::Excel(body) => workbook_state::patch_formula_changes(
                &mut body.workbook,
                &mut self.projection,
                cell_changes,
            )
            .map_err(|error| AppError::WorkbookPatchFailed(error.to_string())),
            SpreadsheetDocumentBody::Csv | SpreadsheetDocumentBody::GeneratedWorkbook => Ok(()),
        }
    }

    fn patch_workbook_layout(&mut self, memento: &LayoutMemento) -> Result<(), AppError> {
        match &mut self.body {
            SpreadsheetDocumentBody::Excel(body) => workbook_state::patch_layout_dimensions(
                &mut body.workbook,
                memento.sheet_index,
                &memento.column_widths,
                &memento.row_heights,
            )
            .map_err(|error| AppError::WorkbookPatchFailed(error.to_string())),
            SpreadsheetDocumentBody::Csv | SpreadsheetDocumentBody::GeneratedWorkbook => Ok(()),
        }
    }

    fn patch_workbook_cell_shapes(&mut self, shapes: &[SheetShapeMemento]) -> Result<(), AppError> {
        match &mut self.body {
            SpreadsheetDocumentBody::Excel(body) => {
                let sheet_shapes: Vec<(usize, Vec<usize>)> = shapes
                    .iter()
                    .map(|shape| (shape.sheet_index, shape.row_lengths.clone()))
                    .collect();
                workbook_state::patch_cell_shapes(&mut body.workbook, &sheet_shapes)
                    .map_err(|error| AppError::WorkbookPatchFailed(error.to_string()))
            }
            SpreadsheetDocumentBody::Csv | SpreadsheetDocumentBody::GeneratedWorkbook => Ok(()),
        }
    }

    fn refresh_projection_from_workbook(&mut self) {
        if let SpreadsheetDocumentBody::Excel(body) = &self.body {
            workbook_state::refresh_projection_from_workbook(&body.workbook, &mut self.projection);
        }
    }

    fn clone_body(&self) -> SpreadsheetDocumentBody {
        match &self.body {
            SpreadsheetDocumentBody::Excel(body) => {
                SpreadsheetDocumentBody::Excel(ExcelDocumentBody {
                    workbook: body.workbook.clone(),
                })
            }
            SpreadsheetDocumentBody::Csv => SpreadsheetDocumentBody::Csv,
            SpreadsheetDocumentBody::GeneratedWorkbook => {
                SpreadsheetDocumentBody::GeneratedWorkbook
            }
        }
    }
}

fn restore_projection_sheets(file_data: &mut FileData, memento: &FileStructureMemento) {
    if memento.sheets.is_empty() {
        file_data.sheets.truncate(memento.sheet_count);
        return;
    }

    let min_index = memento
        .sheets
        .iter()
        .map(|sheet| sheet.sheet_index)
        .min()
        .unwrap_or(0);
    file_data.sheets.truncate(min_index);

    for snapshot in &memento.sheets {
        if file_data.sheets.len() < snapshot.sheet_index {
            file_data
                .sheets
                .resize_with(snapshot.sheet_index, SheetData::default);
        }
        if file_data.sheets.len() == snapshot.sheet_index {
            file_data.sheets.push(snapshot.sheet.clone());
        } else {
            file_data.sheets[snapshot.sheet_index] = snapshot.sheet.clone();
        }
    }

    file_data.sheets.truncate(memento.sheet_count);
}

fn restore_workbook_sheets(
    workbook: &mut Workbook,
    sheet_count: usize,
    sheets: &[WorksheetSnapshot],
) -> Result<(), AppError> {
    while workbook.sheet_count() > sheet_count {
        workbook
            .remove_sheet(workbook.sheet_count() - 1)
            .map_err(|error| AppError::WorkbookPatchFailed(error.to_string()))?;
    }

    {
        let workbook_sheets = workbook.sheet_collection_mut();
        for snapshot in sheets {
            if let Some(target) = workbook_sheets.get_mut(snapshot.sheet_index) {
                *target = snapshot.worksheet.clone();
            }
        }
    }

    Ok(())
}

fn push_unique_position(
    positions: &mut Vec<(usize, usize, usize)>,
    seen: &mut HashSet<(usize, usize, usize)>,
    sheet_index: usize,
    row: usize,
    col: usize,
) {
    if seen.insert((sheet_index, row, col)) {
        positions.push((sheet_index, row, col));
    }
}

fn ensure_projection_cell_exists(sheet: &mut crate::types::SheetData, row: usize, col: usize) {
    let target_width = col + 1;
    while sheet.rows.len() <= row {
        sheet.rows.push(vec![CellValue::Null; target_width]);
    }
    for row_data in &mut sheet.rows {
        if row_data.len() < target_width {
            row_data.resize(target_width, CellValue::Null);
        }
    }
}

fn is_csv_document(file_data: &FileData) -> bool {
    Path::new(&file_data.file_name)
        .extension()
        .or_else(|| Path::new(&file_data.path).extension())
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
}
