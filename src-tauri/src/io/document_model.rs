use crate::error::AppError;
use crate::formula::engine::{FormulaCellRef, FormulaRuntime};
use crate::io::document_body::{BodyRestoreAction, BodyStructureMemento, SpreadsheetDocumentBody};
use crate::io::workbook_state::StructurePatchDiagnostics;
use crate::ops::AppliedOperation;
use crate::state::content_hash::{ContentHash, hash_file_content};
use crate::types::{
    AppliedOperationResult, CellValue, FileData, MergeRange, SheetCellChange, SheetData,
    SheetRichProjection, WorkbookCapabilities,
};
use crate::types::{FormulaDiagnostics, FormulaStatus};
use std::collections::{HashMap, HashSet};
use umya_spreadsheet::Workbook;

#[derive(Debug, Clone)]
pub struct DocumentOperationResult {
    pub operation: AppliedOperationResult,
    pub cell_changes: Vec<SheetCellChange>,
}

pub(crate) struct DocumentMemento {
    before: DocumentMementoSide,
    after: DocumentMementoSide,
}

impl DocumentMemento {
    pub(crate) fn estimated_bytes(&self) -> usize {
        self.before.estimated_bytes() + self.after.estimated_bytes()
    }
}

pub(crate) enum DocumentMementoSide {
    Cells(CellMemento),
    Layout(LayoutMemento),
    Structure(StructureMemento),
}

impl DocumentMementoSide {
    fn estimated_bytes(&self) -> usize {
        match self {
            DocumentMementoSide::Cells(memento) => memento.estimated_bytes(),
            DocumentMementoSide::Layout(memento) => memento.estimated_bytes(),
            DocumentMementoSide::Structure(memento) => memento.estimated_bytes(),
        }
    }
}

pub(crate) struct CellMemento {
    cells: Vec<SheetCellChange>,
    sheet_shapes: Vec<SheetShapeMemento>,
}

impl CellMemento {
    fn estimated_bytes(&self) -> usize {
        self.cells
            .iter()
            .map(estimate_sheet_cell_change_bytes)
            .sum::<usize>()
            + self
                .sheet_shapes
                .iter()
                .map(SheetShapeMemento::estimated_bytes)
                .sum::<usize>()
    }
}

struct SheetShapeMemento {
    sheet_index: usize,
    row_lengths: Vec<usize>,
}

impl SheetShapeMemento {
    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.row_lengths.len() * std::mem::size_of::<usize>()
    }
}

pub(crate) struct LayoutMemento {
    sheet_index: usize,
    column_widths: HashMap<usize, Option<u32>>,
    row_heights: HashMap<usize, Option<u32>>,
}

impl LayoutMemento {
    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + (self.column_widths.len() + self.row_heights.len()) * 32
    }
}

pub(crate) struct StructureMemento {
    projection: FileStructureMemento,
    body: BodyStructureMemento,
}

impl StructureMemento {
    fn estimated_bytes(&self) -> usize {
        self.projection.estimated_bytes() + self.body.estimated_bytes()
    }
}

pub(crate) enum FileStructureMemento {
    Empty { sheet_count: usize },
    Row(RowStructureMemento),
    Column(ColumnStructureMemento),
    Sheets(SheetTailMemento),
}

impl FileStructureMemento {
    fn empty(sheet_count: usize) -> Self {
        Self::Empty { sheet_count }
    }

    fn estimated_bytes(&self) -> usize {
        match self {
            Self::Empty { .. } => std::mem::size_of::<Self>(),
            Self::Row(memento) => memento.estimated_bytes(),
            Self::Column(memento) => memento.estimated_bytes(),
            Self::Sheets(memento) => memento.estimated_bytes(),
        }
    }
}

pub(crate) struct RowStructureMemento {
    sheet_index: usize,
    row_index: usize,
    row_count: usize,
    row: Option<Vec<CellValue>>,
    merges: Vec<MergeRange>,
    row_heights: Option<HashMap<usize, u32>>,
    rich: SheetRichProjection,
}

impl RowStructureMemento {
    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self
                .row
                .as_ref()
                .map(|row| {
                    std::mem::size_of::<Vec<CellValue>>()
                        + row.iter().map(estimate_cell_value_bytes).sum::<usize>()
                })
                .unwrap_or_default()
            + self.merges.len() * std::mem::size_of::<MergeRange>()
            + self
                .row_heights
                .as_ref()
                .map(|heights| heights.len() * 24)
                .unwrap_or_default()
            + estimate_sheet_rich_projection_bytes(&self.rich)
    }
}

pub(crate) struct ColumnStructureMemento {
    sheet_index: usize,
    col_index: usize,
    row_lengths: Vec<usize>,
    values: Vec<Option<CellValue>>,
    merges: Vec<MergeRange>,
    column_widths: Option<HashMap<usize, u32>>,
    rich: SheetRichProjection,
}

impl ColumnStructureMemento {
    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.row_lengths.len() * std::mem::size_of::<usize>()
            + self
                .values
                .iter()
                .flatten()
                .map(estimate_cell_value_bytes)
                .sum::<usize>()
            + self.merges.len() * std::mem::size_of::<MergeRange>()
            + self
                .column_widths
                .as_ref()
                .map(|widths| widths.len() * 24)
                .unwrap_or_default()
            + estimate_sheet_rich_projection_bytes(&self.rich)
    }
}

pub(crate) struct SheetTailMemento {
    sheet_count: usize,
    truncate_from: usize,
    sheets: Vec<ProjectionSheetSnapshot>,
}

impl SheetTailMemento {
    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self
                .sheets
                .iter()
                .map(ProjectionSheetSnapshot::estimated_bytes)
                .sum::<usize>()
    }
}

pub(crate) struct ProjectionSheetSnapshot {
    sheet_index: usize,
    sheet: SheetData,
}

impl ProjectionSheetSnapshot {
    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + estimate_sheet_data_bytes(&self.sheet)
    }
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
    pending_structure_diagnostics: StructurePatchDiagnostics,
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
        let result = self
            .document
            .apply_operation_to_body_and_projection(self.operation)?;

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
            pending_structure_diagnostics: StructurePatchDiagnostics::default(),
        }
    }
}

impl SpreadsheetDocument {
    pub fn new(mut projection: FileData, workbook: Option<Workbook>) -> Self {
        let (formula_runtime, formula_status) = match FormulaRuntime::new(&mut projection) {
            Ok(runtime) => {
                let status = FormulaStatus::ready(runtime.diagnostics());
                (runtime, status)
            }
            Err(error) => {
                eprintln!("Formula runtime initialization failed: {error}");
                (
                    FormulaRuntime::empty(),
                    FormulaStatus::degraded(error.to_string(), FormulaDiagnostics::default()),
                )
            }
        };

        let body = SpreadsheetDocumentBody::from_projection(&projection, workbook);

        Self {
            projection,
            body,
            formula_runtime,
            formula_status,
            pending_structure_diagnostics: StructurePatchDiagnostics::default(),
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

    pub fn capabilities(&self) -> WorkbookCapabilities {
        self.body.capabilities()
    }

    pub fn generate_file_bytes_for_target(
        &self,
        target_path_or_name: &str,
    ) -> Result<(String, Vec<u8>), AppError> {
        self.body
            .generate_file_bytes_for_target(&self.projection, target_path_or_name)
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
            AppliedOperation::AddRow { .. }
            | AppliedOperation::DeleteRow { .. }
            | AppliedOperation::AddColumn { .. }
            | AppliedOperation::DeleteColumn { .. } => {
                DocumentMementoSide::Structure(self.structure_memento(operation))
            }
            AppliedOperation::AddSheet { .. } | AppliedOperation::DeleteSheet { .. } => {
                DocumentMementoSide::Structure(self.structure_memento(operation))
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
            FormulaStatus::Ready { .. } => self
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

    fn structure_memento(&self, operation: &AppliedOperation) -> StructureMemento {
        let body = self.body.capture_structure_memento();
        let projection = if matches!(body, BodyStructureMemento::ProjectionOnly) {
            self.projection_structure_memento(operation)
        } else {
            FileStructureMemento::empty(self.projection.sheets.len())
        };

        StructureMemento { projection, body }
    }

    fn projection_structure_memento(&self, operation: &AppliedOperation) -> FileStructureMemento {
        let sheet_count = self.projection.sheets.len();
        match operation {
            AppliedOperation::AddRow {
                sheet_index,
                row_index,
                ..
            }
            | AppliedOperation::DeleteRow {
                sheet_index,
                row_index,
            } => self
                .projection
                .sheets
                .get(*sheet_index)
                .map(|sheet| {
                    FileStructureMemento::Row(RowStructureMemento {
                        sheet_index: *sheet_index,
                        row_index: *row_index,
                        row_count: sheet.rows.len(),
                        row: sheet.rows.get(*row_index).cloned(),
                        merges: sheet.merges.clone(),
                        row_heights: sheet.row_heights.clone(),
                        rich: sheet.rich.clone(),
                    })
                })
                .unwrap_or_else(|| FileStructureMemento::empty(sheet_count)),
            AppliedOperation::AddColumn {
                sheet_index,
                col_index,
                ..
            }
            | AppliedOperation::DeleteColumn {
                sheet_index,
                col_index,
            } => self
                .projection
                .sheets
                .get(*sheet_index)
                .map(|sheet| {
                    FileStructureMemento::Column(ColumnStructureMemento {
                        sheet_index: *sheet_index,
                        col_index: *col_index,
                        row_lengths: sheet.rows.iter().map(Vec::len).collect(),
                        values: sheet
                            .rows
                            .iter()
                            .map(|row| row.get(*col_index).cloned())
                            .collect(),
                        merges: sheet.merges.clone(),
                        column_widths: sheet.column_widths.clone(),
                        rich: sheet.rich.clone(),
                    })
                })
                .unwrap_or_else(|| FileStructureMemento::empty(sheet_count)),
            AppliedOperation::AddSheet { sheet_index, .. }
            | AppliedOperation::DeleteSheet { sheet_index } => {
                FileStructureMemento::Sheets(SheetTailMemento {
                    sheet_count,
                    truncate_from: *sheet_index,
                    sheets: (*sheet_index..sheet_count)
                        .filter_map(|sheet_index| {
                            self.projection
                                .sheets
                                .get(sheet_index)
                                .cloned()
                                .map(|sheet| ProjectionSheetSnapshot { sheet_index, sheet })
                        })
                        .collect(),
                })
            }
            AppliedOperation::SetCell { .. }
            | AppliedOperation::SetCells { .. }
            | AppliedOperation::SetColumnWidth { .. }
            | AppliedOperation::SetRowHeight { .. } => FileStructureMemento::empty(sheet_count),
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
        match self.body.restore_structure_memento(&memento.body)? {
            BodyRestoreAction::RefreshProjectionFromWorkbook => {
                self.refresh_projection_from_workbook();
            }
            BodyRestoreAction::RestoreProjectionOnly => {
                restore_projection_structure(&mut self.projection, &memento.projection);
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
                        self.formula_status =
                            FormulaStatus::ready(self.formula_runtime.diagnostics());
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
                        self.formula_status =
                            FormulaStatus::ready(self.formula_runtime.diagnostics());
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
            _ => match self
                .formula_runtime
                .rebuild_with_diagnostics(&mut self.projection)
            {
                Ok(result) => {
                    let mut diagnostics = result.diagnostics;
                    self.merge_structure_diagnostics(&mut diagnostics);
                    self.formula_status = FormulaStatus::ready(diagnostics);
                    result.changes
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
        self.formula_status = FormulaStatus::degraded(error, FormulaDiagnostics::default());
        changes
    }

    fn rebuild_formula_runtime(&mut self) {
        match self
            .formula_runtime
            .rebuild_with_diagnostics(&mut self.projection)
        {
            Ok(result) => {
                let mut diagnostics = result.diagnostics;
                self.merge_structure_diagnostics(&mut diagnostics);
                self.formula_status = FormulaStatus::ready(diagnostics);
            }
            Err(error) => {
                eprintln!("Formula runtime rebuild failed: {error}");
                self.formula_runtime = FormulaRuntime::empty();
                self.formula_status =
                    FormulaStatus::degraded(error.to_string(), FormulaDiagnostics::default());
            }
        }
    }

    fn patch_workbook_after_operation(
        &mut self,
        operation: &AppliedOperation,
        _result: &AppliedOperationResult,
        cell_changes: &[SheetCellChange],
    ) -> Result<(), AppError> {
        self.body
            .patch_after_operation(&mut self.projection, operation, cell_changes)
            .map_err(|error| AppError::WorkbookPatchFailed(error.to_string()))
    }

    fn apply_operation_to_body_and_projection(
        &mut self,
        operation: &AppliedOperation,
    ) -> Result<AppliedOperationResult, AppError> {
        if operation.is_structure_change()
            && let Some(diagnostics) = self.body.apply_structure_operation(operation)?
        {
            self.pending_structure_diagnostics = diagnostics;
            self.refresh_projection_from_workbook();
            self.body
                .sync_all_merge_ranges_from_projection(&self.projection)?;
            self.refresh_projection_from_workbook();
            return Ok(operation.projected_result_from_current_file(&self.projection));
        }

        Ok(operation
            .execute_cells_and_layout(&mut self.projection)
            .unwrap_or_else(|| operation.execute(&mut self.projection)))
    }

    fn patch_workbook_formula_changes(
        &mut self,
        cell_changes: &[SheetCellChange],
    ) -> Result<(), AppError> {
        self.body
            .patch_formula_changes(&mut self.projection, cell_changes)
            .map_err(|error| AppError::WorkbookPatchFailed(error.to_string()))
    }

    fn patch_workbook_layout(&mut self, memento: &LayoutMemento) -> Result<(), AppError> {
        self.body
            .patch_layout_dimensions(
                memento.sheet_index,
                &memento.column_widths,
                &memento.row_heights,
            )
            .map_err(|error| AppError::WorkbookPatchFailed(error.to_string()))
    }

    fn patch_workbook_cell_shapes(&mut self, shapes: &[SheetShapeMemento]) -> Result<(), AppError> {
        let sheet_shapes: Vec<(usize, Vec<usize>)> = shapes
            .iter()
            .map(|shape| (shape.sheet_index, shape.row_lengths.clone()))
            .collect();
        self.body
            .patch_cell_shapes(&sheet_shapes)
            .map_err(|error| AppError::WorkbookPatchFailed(error.to_string()))
    }

    fn refresh_projection_from_workbook(&mut self) {
        self.body
            .refresh_projection_from_workbook(&mut self.projection);
    }

    fn clone_body(&self) -> SpreadsheetDocumentBody {
        self.body.clone_body()
    }

    fn merge_structure_diagnostics(&mut self, diagnostics: &mut FormulaDiagnostics) {
        diagnostics.skipped_reference_rewrite_count += self
            .pending_structure_diagnostics
            .skipped_formula_reference_rewrites;
        self.pending_structure_diagnostics = StructurePatchDiagnostics::default();
    }
}

fn restore_projection_structure(file_data: &mut FileData, memento: &FileStructureMemento) {
    match memento {
        FileStructureMemento::Empty { sheet_count } => {
            file_data.sheets.truncate(*sheet_count);
        }
        FileStructureMemento::Row(memento) => restore_projection_row(file_data, memento),
        FileStructureMemento::Column(memento) => restore_projection_column(file_data, memento),
        FileStructureMemento::Sheets(memento) => restore_projection_sheet_tail(file_data, memento),
    }
}

fn restore_projection_row(file_data: &mut FileData, memento: &RowStructureMemento) {
    let Some(sheet) = file_data.sheets.get_mut(memento.sheet_index) else {
        return;
    };
    if sheet.rows.len() > memento.row_count {
        if memento.row_index < sheet.rows.len() {
            sheet.rows.remove(memento.row_index);
        }
    } else if sheet.rows.len() < memento.row_count {
        let row = memento.row.clone().unwrap_or_default();
        sheet
            .rows
            .insert(memento.row_index.min(sheet.rows.len()), row);
    } else if let Some(row) = &memento.row
        && memento.row_index < sheet.rows.len()
    {
        sheet.rows[memento.row_index] = row.clone();
    }

    sheet.rows.truncate(memento.row_count);
    sheet.merges = memento.merges.clone();
    sheet.row_heights = memento.row_heights.clone();
    sheet.rich = memento.rich.clone();
}

fn restore_projection_column(file_data: &mut FileData, memento: &ColumnStructureMemento) {
    let Some(sheet) = file_data.sheets.get_mut(memento.sheet_index) else {
        return;
    };

    if sheet.rows.len() < memento.row_lengths.len() {
        sheet.rows.resize_with(memento.row_lengths.len(), Vec::new);
    }
    sheet.rows.truncate(memento.row_lengths.len());

    for (row_index, target_len) in memento.row_lengths.iter().copied().enumerate() {
        let row = &mut sheet.rows[row_index];
        let value = memento.values.get(row_index).cloned().flatten();
        if row.len() > target_len {
            if memento.col_index < row.len() {
                row.remove(memento.col_index);
            }
        } else if row.len() < target_len {
            row.insert(
                memento.col_index.min(row.len()),
                value.unwrap_or(CellValue::Null),
            );
        } else if let Some(value) = value
            && memento.col_index < row.len()
        {
            row[memento.col_index] = value;
        }
        row.truncate(target_len);
    }

    sheet.merges = memento.merges.clone();
    sheet.column_widths = memento.column_widths.clone();
    sheet.rich = memento.rich.clone();
}

fn restore_projection_sheet_tail(file_data: &mut FileData, memento: &SheetTailMemento) {
    file_data.sheets.truncate(memento.truncate_from);

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

fn estimate_sheet_cell_change_bytes(change: &SheetCellChange) -> usize {
    std::mem::size_of::<SheetCellChange>() + estimate_cell_value_bytes(&change.value)
}

fn estimate_sheet_data_bytes(sheet: &SheetData) -> usize {
    std::mem::size_of::<SheetData>()
        + sheet.name.len()
        + sheet
            .rows
            .iter()
            .map(|row| {
                std::mem::size_of::<Vec<CellValue>>()
                    + row.iter().map(estimate_cell_value_bytes).sum::<usize>()
            })
            .sum::<usize>()
        + sheet.merges.len() * std::mem::size_of::<crate::types::MergeRange>()
        + sheet
            .column_widths
            .as_ref()
            .map(|widths| widths.len() * 24)
            .unwrap_or_default()
        + sheet
            .row_heights
            .as_ref()
            .map(|heights| heights.len() * 24)
            .unwrap_or_default()
        + sheet
            .rich
            .cell_styles
            .iter()
            .map(|(cell, style)| cell.len() + estimate_cell_style_projection_bytes(style))
            .sum::<usize>()
        + sheet.rich.drawings.len() * std::mem::size_of::<crate::types::DrawingProjection>()
}

fn estimate_sheet_rich_projection_bytes(rich: &SheetRichProjection) -> usize {
    std::mem::size_of::<SheetRichProjection>()
        + rich
            .cell_styles
            .iter()
            .map(|(cell, style)| cell.len() + estimate_cell_style_projection_bytes(style))
            .sum::<usize>()
        + rich.drawings.len() * std::mem::size_of::<crate::types::DrawingProjection>()
}

fn estimate_cell_style_projection_bytes(style: &crate::types::CellStyleProjection) -> usize {
    style
        .font_color
        .as_ref()
        .map(String::len)
        .unwrap_or_default()
        + style
            .background_color
            .as_ref()
            .map(String::len)
            .unwrap_or_default()
        + style
            .horizontal_align
            .as_ref()
            .map(String::len)
            .unwrap_or_default()
        + style
            .vertical_align
            .as_ref()
            .map(String::len)
            .unwrap_or_default()
        + style
            .number_format
            .as_ref()
            .map(String::len)
            .unwrap_or_default()
        + std::mem::size_of::<crate::types::CellStyleProjection>()
}

fn estimate_cell_value_bytes(cell: &CellValue) -> usize {
    match cell {
        CellValue::Null | CellValue::Boolean(_) => std::mem::size_of::<CellValue>(),
        CellValue::String(value) => std::mem::size_of::<CellValue>() + value.len(),
        CellValue::Number(value) => std::mem::size_of::<CellValue>() + value.to_string().len(),
        CellValue::Formula {
            formula,
            cached_value,
            error,
        } => {
            std::mem::size_of::<CellValue>()
                + formula.len()
                + estimate_cell_value_bytes(cached_value)
                + error.as_ref().map(String::len).unwrap_or_default()
        }
    }
}
