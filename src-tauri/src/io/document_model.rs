use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::error::AppError;
use crate::formula::engine::FormulaRuntime;
use crate::io::codec::writer;
use crate::io::workbook_state;
use crate::ops::AppliedOperation;
use crate::state::content_hash::{ContentHash, hash_file_content};
use crate::types::{CellValue, FileData, OperationResult, SheetCellChange};
use umya_spreadsheet::Workbook;

#[derive(Debug, Clone)]
pub struct DocumentOperationResult {
    pub operation: OperationResult,
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

pub(crate) enum DocumentMemento {
    Cells {
        before: CellMemento,
        after: CellMemento,
    },
    Layout {
        before: LayoutMemento,
        after: LayoutMemento,
    },
    Full {
        before: Box<SpreadsheetDocument>,
        after: Box<SpreadsheetDocument>,
    },
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
        }
    }
}

impl SpreadsheetDocument {
    pub fn new(mut projection: FileData, workbook: Option<Workbook>) -> Self {
        let formula_runtime = FormulaRuntime::new(&mut projection).unwrap_or_else(|error| {
            eprintln!("Formula runtime initialization failed: {error}");
            FormulaRuntime::empty()
        });

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
        }
    }

    pub fn projection(&self) -> &FileData {
        &self.projection
    }

    pub fn content_hash(&self) -> ContentHash {
        hash_file_content(&self.projection)
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
            "xlsx" | "xlsm" => match &self.body {
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
    ) -> Result<DocumentOperationResult, AppError> {
        let previous_projection = self.projection.clone();
        let previous_body = self.clone_body();

        let result = operation.execute(&mut self.projection);

        if let Err(error) = self.patch_workbook_after_operation(operation, &result, &[]) {
            self.projection = previous_projection;
            self.body = previous_body;
            self.rebuild_formula_runtime();
            return Err(error);
        }

        let cell_changes = self.recalculate_after_operation(operation);

        if !cell_changes.is_empty()
            && let Err(error) = self.patch_workbook_formula_changes(&cell_changes)
        {
            self.projection = previous_projection;
            self.body = previous_body;
            self.rebuild_formula_runtime();
            return Err(error);
        }

        Ok(DocumentOperationResult {
            operation: result,
            cell_changes,
        })
    }

    pub fn create_memento(
        before: &Self,
        after: &Self,
        operation: &AppliedOperation,
        cell_changes: &[SheetCellChange],
    ) -> DocumentMemento {
        match operation {
            AppliedOperation::SetCell {
                sheet_index,
                row,
                col,
                ..
            } => DocumentMemento::Cells {
                before: before.cell_memento(*sheet_index, *row, *col, cell_changes),
                after: after.cell_memento(*sheet_index, *row, *col, cell_changes),
            },
            AppliedOperation::SetColumnWidth {
                sheet_index,
                col_index,
                ..
            } => DocumentMemento::Layout {
                before: before.layout_memento(*sheet_index, Some(*col_index), None),
                after: after.layout_memento(*sheet_index, Some(*col_index), None),
            },
            AppliedOperation::SetRowHeight {
                sheet_index,
                row_index,
                ..
            } => DocumentMemento::Layout {
                before: before.layout_memento(*sheet_index, None, Some(*row_index)),
                after: after.layout_memento(*sheet_index, None, Some(*row_index)),
            },
            AppliedOperation::AddRow { .. }
            | AppliedOperation::DeleteRow { .. }
            | AppliedOperation::AddColumn { .. }
            | AppliedOperation::DeleteColumn { .. }
            | AppliedOperation::AddSheet { .. }
            | AppliedOperation::DeleteSheet { .. } => DocumentMemento::Full {
                before: Box::new(before.clone()),
                after: Box::new(after.clone()),
            },
        }
    }

    pub fn restore_memento(
        &mut self,
        memento: &DocumentMemento,
        side: MementoSide,
    ) -> Result<(), AppError> {
        match (memento, side) {
            (DocumentMemento::Cells { before, .. }, MementoSide::Before) => {
                self.restore_cells(before)
            }
            (DocumentMemento::Cells { after, .. }, MementoSide::After) => self.restore_cells(after),
            (DocumentMemento::Layout { before, .. }, MementoSide::Before) => {
                self.restore_layout(before)
            }
            (DocumentMemento::Layout { after, .. }, MementoSide::After) => {
                self.restore_layout(after)
            }
            (DocumentMemento::Full { before, .. }, MementoSide::Before) => {
                *self = (**before).clone();
                Ok(())
            }
            (DocumentMemento::Full { after, .. }, MementoSide::After) => {
                *self = (**after).clone();
                Ok(())
            }
        }
    }

    fn cell_memento(
        &self,
        sheet_index: usize,
        row: usize,
        col: usize,
        cell_changes: &[SheetCellChange],
    ) -> CellMemento {
        let mut positions = Vec::new();
        let mut seen = HashSet::new();
        push_unique_position(&mut positions, &mut seen, sheet_index, row, col);
        for change in cell_changes {
            push_unique_position(
                &mut positions,
                &mut seen,
                change.sheet_index,
                change.row,
                change.col,
            );
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
                    Ok(changes) => changes,
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
            AppliedOperation::SetColumnWidth { .. } | AppliedOperation::SetRowHeight { .. } => {
                Vec::new()
            }
            _ => match self.formula_runtime.rebuild(&mut self.projection) {
                Ok(changes) => changes,
                Err(error) => {
                    eprintln!("Formula recalculation failed: {error}");
                    self.rebuild_formula_runtime();
                    Vec::new()
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

    fn rebuild_formula_runtime(&mut self) {
        if let Err(error) = self.formula_runtime.rebuild(&mut self.projection) {
            eprintln!("Formula runtime rebuild failed: {error}");
            self.formula_runtime = FormulaRuntime::empty();
        }
    }

    fn patch_workbook_after_operation(
        &mut self,
        operation: &AppliedOperation,
        result: &OperationResult,
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
