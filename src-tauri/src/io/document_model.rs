use crate::error::AppError;
use crate::formula::cell_ref::FormulaCellRef;
use crate::io::document_body::BodySheetShape;
use crate::io::document_body::{BodyRestoreAction, BodyStructureMemento, SpreadsheetDocumentBody};
use crate::io::document_patches::{CurrentStructureShape, restore_structure_patches};
use crate::io::document_transaction::DocumentTransaction;
use crate::io::formula_coordinator::FormulaCoordinator;
use crate::ops::AppliedOperation;
use crate::state::content_hash::{ContentFingerprint, ContentHash, hash_content_fingerprint};
use crate::types::FormulaStatus;
use crate::types::{
    AppliedOperationResult, CellValue, DrawingProjection, EditorPatch, FileData, LayoutPatch,
    MergeRange, ReadOnlyRichProjection, ResyncRequiredPatch, SheetCellChange, SheetData,
    SheetShapePatch, WorkbookCapabilities,
};
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
    Structure(Box<StructureMemento>),
}

impl DocumentMementoSide {
    pub(crate) fn estimated_bytes(&self) -> usize {
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
    protected_cells: Vec<(usize, usize)>,
}

impl SheetShapeMemento {
    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.row_lengths.len() * std::mem::size_of::<usize>()
            + self.protected_cells.len() * std::mem::size_of::<(usize, usize)>()
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
    pub(crate) sheet_index: usize,
    pub(crate) row_index: usize,
    pub(crate) row_count: usize,
    row: Option<Vec<CellValue>>,
    merges: Vec<MergeRange>,
    row_heights: Option<HashMap<usize, u32>>,
    rich: RichProjectionMemento,
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
            + self.rich.estimated_bytes()
    }
}

pub(crate) struct ColumnStructureMemento {
    pub(crate) sheet_index: usize,
    pub(crate) col_index: usize,
    pub(crate) row_lengths: Vec<usize>,
    values: Vec<Option<CellValue>>,
    merges: Vec<MergeRange>,
    column_widths: Option<HashMap<usize, u32>>,
    rich: RichProjectionMemento,
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
            + self.rich.estimated_bytes()
    }
}

pub(crate) struct RichProjectionMemento {
    scope: RichProjectionScope,
    projection: ReadOnlyRichProjection,
}

impl RichProjectionMemento {
    fn row_tail(source: &ReadOnlyRichProjection, row_index: usize) -> Self {
        Self {
            scope: RichProjectionScope::Rows { start: row_index },
            projection: filter_rich_projection(
                source,
                |row, _| row >= row_index,
                |row| row >= row_index,
                |_| false,
                |drawing| drawing_row_scope_affected(drawing, row_index),
            ),
        }
    }

    fn column_tail(source: &ReadOnlyRichProjection, col_index: usize) -> Self {
        Self {
            scope: RichProjectionScope::Columns { start: col_index },
            projection: filter_rich_projection(
                source,
                |_, col| col >= col_index,
                |_| false,
                |col| col >= col_index,
                |drawing| drawing_column_scope_affected(drawing, col_index),
            ),
        }
    }

    fn restore_into(&self, target: &mut ReadOnlyRichProjection) {
        match self.scope {
            RichProjectionScope::Rows { start } => {
                target
                    .cell_formats
                    .retain(|key, _| !cell_key_matches(key, |row, _| row >= start));
                target
                    .cell_styles
                    .retain(|key, _| !cell_key_matches(key, |row, _| row >= start));
                target
                    .drawings
                    .retain(|drawing| !drawing_row_scope_affected(drawing, start));
                target.hidden_rows.retain(|row| *row < start);
                target
                    .hyperlinks
                    .retain(|key, _| !cell_key_matches(key, |row, _| row >= start));
            }
            RichProjectionScope::Columns { start } => {
                target
                    .cell_formats
                    .retain(|key, _| !cell_key_matches(key, |_, col| col >= start));
                target
                    .cell_styles
                    .retain(|key, _| !cell_key_matches(key, |_, col| col >= start));
                target
                    .drawings
                    .retain(|drawing| !drawing_column_scope_affected(drawing, start));
                target.hidden_columns.retain(|col| *col < start);
                target
                    .hyperlinks
                    .retain(|key, _| !cell_key_matches(key, |_, col| col >= start));
            }
        }

        target
            .cell_formats
            .extend(self.projection.cell_formats.clone());
        target
            .cell_styles
            .extend(self.projection.cell_styles.clone());
        target
            .hidden_rows
            .extend(self.projection.hidden_rows.iter().copied());
        target.hidden_rows.sort_unstable();
        target.hidden_rows.dedup();
        target
            .hidden_columns
            .extend(self.projection.hidden_columns.iter().copied());
        target.hidden_columns.sort_unstable();
        target.hidden_columns.dedup();
        target.freeze_pane = self.projection.freeze_pane.clone();
        target.hyperlinks.extend(self.projection.hyperlinks.clone());
        target.drawings.extend(self.projection.drawings.clone());
        target.has_more_drawings = self.projection.has_more_drawings;
    }

    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + estimate_sheet_rich_projection_bytes(&self.projection)
    }
}

#[derive(Clone, Copy)]
enum RichProjectionScope {
    Rows { start: usize },
    Columns { start: usize },
}

pub(crate) struct SheetTailMemento {
    pub(crate) sheet_count: usize,
    pub(crate) truncate_from: usize,
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

#[derive(Clone, Copy)]
pub(crate) enum MementoSide {
    Before,
    After,
}

#[derive(Debug, Clone)]
pub struct DocumentRestoreResult {
    pub patches: Vec<EditorPatch>,
}

impl DocumentRestoreResult {
    fn empty() -> Self {
        Self {
            patches: Vec::new(),
        }
    }
}

/// Canonical spreadsheet document.
///
/// Excel files keep the original `Workbook` as the persistence object. `FileData`
/// is a projection used by UI, formula calculation, search, and dirty hashing.
pub struct SpreadsheetDocument {
    projection: FileData,
    body: SpreadsheetDocumentBody,
    cached_capabilities: WorkbookCapabilities,
    formulas: FormulaCoordinator,
    transaction_failure: Option<String>,
}

impl SpreadsheetDocument {
    pub fn new(mut projection: FileData, workbook: Option<Workbook>) -> Self {
        let formulas = FormulaCoordinator::new(&mut projection);
        let body = SpreadsheetDocumentBody::from_projection(&projection, workbook);
        let cached_capabilities = body.capabilities();

        Self {
            projection,
            body,
            cached_capabilities,
            formulas,
            transaction_failure: None,
        }
    }

    pub fn projection(&self) -> &FileData {
        &self.projection
    }

    pub fn update_identity(&mut self, path: String, file_name: String) {
        self.projection.path = path;
        self.projection.file_name = file_name;
    }

    pub fn content_hash(&self) -> ContentHash {
        hash_content_fingerprint(&ContentFingerprint::from_file_data(&self.projection))
    }

    pub fn formula_status(&self) -> FormulaStatus {
        self.formulas.status()
    }

    pub fn capabilities(&self) -> WorkbookCapabilities {
        let mut capabilities = self.cached_capabilities.clone();
        if let Some(reason) = &self.transaction_failure {
            capabilities.can_edit_cells = false;
            capabilities.can_resize_rows_columns = false;
            capabilities.can_insert_delete_rows = false;
            capabilities.can_insert_delete_columns = false;
            capabilities.can_insert_delete_sheets = false;
            capabilities.can_native_save = false;
            push_unique_reason(&mut capabilities.blocked_edit_reasons, reason);
            push_unique_reason(&mut capabilities.blocked_resize_reasons, reason);
            push_unique_reason(&mut capabilities.blocked_row_structure_reasons, reason);
            push_unique_reason(&mut capabilities.blocked_column_structure_reasons, reason);
            push_unique_reason(&mut capabilities.blocked_sheet_structure_reasons, reason);
            push_unique_reason(&mut capabilities.blocked_structure_reasons, reason);
            push_unique_reason(
                &mut capabilities.detected_features,
                "failed editor transaction",
            );
        }
        capabilities
    }

    pub fn generate_file_bytes_for_target(
        &self,
        target_path_or_name: &str,
    ) -> Result<(String, Vec<u8>), AppError> {
        if let Some(reason) = &self.transaction_failure {
            return Err(AppError::DocumentStateInvalid(reason.clone()));
        }
        self.validate_persisted_projection_consistency()?;
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
                DocumentMementoSide::Structure(Box::new(self.structure_memento(operation)))
            }
            AppliedOperation::AddSheet { .. } | AppliedOperation::DeleteSheet { .. } => {
                DocumentMementoSide::Structure(Box::new(self.structure_memento(operation)))
            }
        }
    }

    pub fn restore_memento(
        &mut self,
        memento: &DocumentMemento,
        side: MementoSide,
    ) -> Result<DocumentRestoreResult, AppError> {
        match side {
            MementoSide::Before => self.restore_memento_side(&memento.before),
            MementoSide::After => self.restore_memento_side(&memento.after),
        }
    }

    pub(crate) fn restore_memento_side(
        &mut self,
        side: &DocumentMementoSide,
    ) -> Result<DocumentRestoreResult, AppError> {
        match side {
            DocumentMementoSide::Cells(memento) => self.restore_cells(memento),
            DocumentMementoSide::Layout(memento) => self.restore_layout(memento),
            DocumentMementoSide::Structure(memento) => self.restore_structure(memento),
        }
    }

    fn cell_memento(&self, changed_cells: impl IntoIterator<Item = FormulaCellRef>) -> CellMemento {
        let changed_cells: Vec<FormulaCellRef> = changed_cells.into_iter().collect();
        let formula_cells = self
            .formulas
            .impacted_cells_for_memento(changed_cells.iter().copied(), &self.projection);
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
                protected_cells: self
                    .projection
                    .sheets
                    .get(sheet_index)
                    .map(protected_rich_cell_positions)
                    .unwrap_or_default(),
            })
            .collect();

        let cells = positions
            .into_iter()
            .map(|(sheet_index, row, col)| {
                SheetCellChange::new(
                    sheet_index,
                    row,
                    col,
                    self.projection_cell(sheet_index, row, col),
                )
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

    fn structure_memento(&self, operation: &AppliedOperation) -> StructureMemento {
        let body = self.body.capture_structure_memento(operation);
        let projection = self.projection_structure_memento(operation);

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
                        rich: RichProjectionMemento::row_tail(&sheet.rich, *row_index),
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
                        rich: RichProjectionMemento::column_tail(&sheet.rich, *col_index),
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

    fn restore_cells(&mut self, memento: &CellMemento) -> Result<DocumentRestoreResult, AppError> {
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
        let formula_changes = self.formulas.rebuild(&mut self.projection);
        if !formula_changes.is_empty() {
            self.patch_workbook_formula_changes(&formula_changes)?;
        }
        self.validate_projection_consistency()?;
        let mut patches = Vec::new();
        let mut cell_changes = memento.cells.clone();
        for change in formula_changes {
            push_sheet_cell_change_if_missing(&mut cell_changes, change);
        }
        if !cell_changes.is_empty() {
            patches.push(EditorPatch::Cells {
                changes: cell_changes,
            });
        }
        patches.extend(shape_restore_patches(&memento.sheet_shapes));
        Ok(DocumentRestoreResult { patches })
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

    fn restore_layout(
        &mut self,
        memento: &LayoutMemento,
    ) -> Result<DocumentRestoreResult, AppError> {
        let Some(sheet) = self.projection.sheets.get_mut(memento.sheet_index) else {
            return Ok(DocumentRestoreResult::empty());
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

        self.patch_workbook_layout(memento)?;
        self.validate_projection_consistency()?;
        Ok(DocumentRestoreResult {
            patches: vec![EditorPatch::Layout {
                patch: LayoutPatch {
                    sheet_index: memento.sheet_index,
                    column_widths: memento.column_widths.clone(),
                    row_heights: memento.row_heights.clone(),
                },
            }],
        })
    }

    fn restore_structure(
        &mut self,
        memento: &StructureMemento,
    ) -> Result<DocumentRestoreResult, AppError> {
        let current_shape = CurrentStructureShape::capture(&self.projection, &memento.projection);
        match self.body.restore_structure_memento(&memento.body)? {
            BodyRestoreAction::RefreshProjectionFromWorkbook => {
                self.refresh_projection_from_workbook();
            }
            BodyRestoreAction::RestoreProjectionOnly => {
                restore_projection_structure(&mut self.projection, &memento.projection);
            }
        }
        self.refresh_capabilities();
        let formula_changes = self.formulas.rebuild(&mut self.projection);
        if !formula_changes.is_empty() {
            self.patch_workbook_formula_changes(&formula_changes)?;
        }
        self.validate_persisted_projection_consistency()?;
        self.validate_projection_consistency()?;
        let mut patches =
            restore_structure_patches(&current_shape, &memento.projection, &self.projection);
        if patches.is_empty() {
            patches.push(EditorPatch::ResyncRequired {
                patch: ResyncRequiredPatch {
                    reason: "structure restore changed workbook projection".to_string(),
                },
            });
        }
        if !formula_changes.is_empty() {
            patches.push(EditorPatch::Cells {
                changes: formula_changes,
            });
        }
        Ok(DocumentRestoreResult { patches })
    }

    pub(crate) fn recalculate_after_operation(
        &mut self,
        operation: &AppliedOperation,
    ) -> Vec<SheetCellChange> {
        self.formulas
            .recalculate_after_operation(operation, &mut self.projection)
    }

    pub(crate) fn patch_workbook_after_operation(
        &mut self,
        operation: &AppliedOperation,
        _result: &AppliedOperationResult,
        cell_changes: &[SheetCellChange],
    ) -> Result<(), AppError> {
        self.body
            .patch_after_operation(&mut self.projection, operation, cell_changes)
            .map_err(|error| AppError::WorkbookPatchFailed(error.to_string()))
    }

    pub(crate) fn apply_operation_to_body_and_projection(
        &mut self,
        operation: &AppliedOperation,
    ) -> Result<AppliedOperationResult, AppError> {
        if let Some(result) = self
            .body
            .apply_structure_operation(&mut self.projection, operation)?
        {
            self.formulas
                .set_pending_structure_diagnostics(result.diagnostics);
            self.refresh_capabilities();
            self.validate_persisted_projection_consistency()?;
            return Ok(result.result);
        }

        Ok(operation
            .projection_mutation()
            .execute_cells_and_layout(&mut self.projection)
            .unwrap_or_else(|| {
                operation
                    .projection_mutation()
                    .execute(&mut self.projection)
            }))
    }

    pub(crate) fn patch_workbook_formula_changes(
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
        let sheet_shapes: Vec<BodySheetShape> = shapes
            .iter()
            .map(|shape| BodySheetShape {
                sheet_index: shape.sheet_index,
                row_lengths: shape.row_lengths.clone(),
                protected_cells: shape.protected_cells.clone(),
            })
            .collect();
        self.body
            .patch_cell_shapes(&sheet_shapes)
            .map_err(|error| AppError::WorkbookPatchFailed(error.to_string()))
    }

    fn refresh_projection_from_workbook(&mut self) {
        self.body
            .refresh_projection_from_workbook(&mut self.projection);
    }

    fn refresh_capabilities(&mut self) {
        self.cached_capabilities = self.body.capabilities();
    }

    pub(crate) fn validate_projection_consistency(&self) -> Result<(), AppError> {
        self.body.validate_projection_consistency(&self.projection)
    }

    pub(crate) fn validate_persisted_projection_consistency(&self) -> Result<(), AppError> {
        self.body
            .validate_persisted_projection_consistency(&self.projection)
    }

    pub fn transaction_failure(&self) -> Option<&str> {
        self.transaction_failure.as_deref()
    }

    pub(crate) fn mark_transaction_failed(&mut self, reason: String) {
        self.refresh_capabilities();
        self.transaction_failure = Some(reason.clone());
        self.formulas.mark_degraded(reason);
    }
}

fn push_unique_reason(reasons: &mut Vec<String>, reason: impl Into<String>) {
    let reason = reason.into();
    if !reasons.iter().any(|existing| existing == &reason) {
        reasons.push(reason);
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
    memento.rich.restore_into(&mut sheet.rich);
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
    memento.rich.restore_into(&mut sheet.rich);
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

fn filter_rich_projection(
    source: &ReadOnlyRichProjection,
    cell_matches: impl Fn(usize, usize) -> bool,
    row_matches: impl Fn(usize) -> bool,
    column_matches: impl Fn(usize) -> bool,
    drawing_matches: impl Fn(&DrawingProjection) -> bool,
) -> ReadOnlyRichProjection {
    ReadOnlyRichProjection {
        cell_formats: filter_cell_projection_map(&source.cell_formats, &cell_matches),
        cell_styles: filter_cell_projection_map(&source.cell_styles, &cell_matches),
        hidden_rows: source
            .hidden_rows
            .iter()
            .copied()
            .filter(|row| row_matches(*row))
            .collect(),
        hidden_columns: source
            .hidden_columns
            .iter()
            .copied()
            .filter(|column| column_matches(*column))
            .collect(),
        freeze_pane: source.freeze_pane.clone(),
        hyperlinks: filter_cell_projection_map(&source.hyperlinks, &cell_matches),
        drawings: source
            .drawings
            .iter()
            .filter(|drawing| drawing_matches(drawing))
            .cloned()
            .collect(),
        has_more_drawings: source.has_more_drawings,
    }
}

fn filter_cell_projection_map<T: Clone>(
    source: &HashMap<String, T>,
    cell_matches: &impl Fn(usize, usize) -> bool,
) -> HashMap<String, T> {
    source
        .iter()
        .filter(|(key, _)| cell_key_matches(key, cell_matches))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn cell_key_matches(key: &str, matches: impl Fn(usize, usize) -> bool) -> bool {
    parse_projection_cell_key(key).is_some_and(|(row, col)| matches(row, col))
}

fn parse_projection_cell_key(key: &str) -> Option<(usize, usize)> {
    let mut col = 0usize;
    let mut row = 0usize;
    let mut saw_digit = false;
    for byte in key.bytes() {
        if byte.is_ascii_alphabetic() && !saw_digit {
            col = col
                .checked_mul(26)?
                .checked_add(usize::from(byte.to_ascii_uppercase() - b'A' + 1))?;
        } else if byte.is_ascii_digit() {
            saw_digit = true;
            row = row.checked_mul(10)?.checked_add(usize::from(byte - b'0'))?;
        } else {
            return None;
        }
    }
    (col > 0 && row > 0).then_some((row - 1, col - 1))
}

fn protected_rich_cell_positions(sheet: &crate::types::SheetData) -> Vec<(usize, usize)> {
    let mut positions = Vec::new();
    let mut seen = HashSet::new();
    for key in sheet
        .rich
        .cell_formats
        .keys()
        .chain(sheet.rich.cell_styles.keys())
        .chain(sheet.rich.hyperlinks.keys())
    {
        if let Some((row, col)) = parse_projection_cell_key(key)
            && seen.insert((row, col))
        {
            positions.push((row, col));
        }
    }
    for drawing in &sheet.rich.drawings {
        push_unique_position_2d(
            &mut positions,
            &mut seen,
            drawing.from_row as usize,
            drawing.from_col as usize,
        );
        if let (Some(row), Some(col)) = (drawing.to_row, drawing.to_col) {
            push_unique_position_2d(&mut positions, &mut seen, row as usize, col as usize);
        }
    }
    positions
}

fn drawing_row_scope_affected(drawing: &DrawingProjection, row_index: usize) -> bool {
    drawing.from_row as usize >= row_index
        || drawing
            .to_row
            .is_some_and(|to_row| to_row as usize >= row_index)
}

fn drawing_column_scope_affected(drawing: &DrawingProjection, col_index: usize) -> bool {
    drawing.from_col as usize >= col_index
        || drawing
            .to_col
            .is_some_and(|to_col| to_col as usize >= col_index)
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

fn push_unique_position_2d(
    positions: &mut Vec<(usize, usize)>,
    seen: &mut HashSet<(usize, usize)>,
    row: usize,
    col: usize,
) {
    if seen.insert((row, col)) {
        positions.push((row, col));
    }
}

fn push_sheet_cell_change_if_missing(changes: &mut Vec<SheetCellChange>, change: SheetCellChange) {
    if !changes.iter().any(|existing| {
        existing.sheet_index == change.sheet_index
            && existing.row == change.row
            && existing.col == change.col
    }) {
        changes.push(change);
    }
}

fn shape_restore_patches(shapes: &[SheetShapeMemento]) -> Vec<EditorPatch> {
    shapes
        .iter()
        .map(|shape| EditorPatch::SheetShape {
            patch: SheetShapePatch {
                sheet_index: shape.sheet_index,
                row_lengths: shape.row_lengths.clone(),
            },
        })
        .collect()
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
        + estimate_sheet_rich_projection_bytes(&sheet.rich)
}

fn estimate_sheet_rich_projection_bytes(rich: &ReadOnlyRichProjection) -> usize {
    std::mem::size_of::<ReadOnlyRichProjection>()
        + rich
            .cell_formats
            .iter()
            .map(|(cell, format)| cell.len() + estimate_cell_format_projection_bytes(format))
            .sum::<usize>()
        + rich
            .cell_styles
            .iter()
            .map(|(cell, style)| cell.len() + estimate_cell_style_projection_bytes(style))
            .sum::<usize>()
        + rich.hidden_rows.len() * std::mem::size_of::<usize>()
        + rich.hidden_columns.len() * std::mem::size_of::<usize>()
        + rich
            .freeze_pane
            .as_ref()
            .map(estimate_freeze_pane_projection_bytes)
            .unwrap_or_default()
        + rich
            .hyperlinks
            .iter()
            .map(|(cell, hyperlink)| cell.len() + estimate_hyperlink_projection_bytes(hyperlink))
            .sum::<usize>()
        + rich.drawings.len() * std::mem::size_of::<crate::types::DrawingProjection>()
}

fn estimate_cell_format_projection_bytes(format: &crate::types::CellFormatProjection) -> usize {
    std::mem::size_of::<crate::types::CellFormatProjection>()
        + format
            .number_format
            .as_ref()
            .map(String::len)
            .unwrap_or_default()
        + format
            .style_id
            .as_ref()
            .map(String::len)
            .unwrap_or_default()
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

fn estimate_freeze_pane_projection_bytes(
    freeze_pane: &crate::types::FreezePaneProjection,
) -> usize {
    std::mem::size_of::<crate::types::FreezePaneProjection>()
        + freeze_pane.top_left_cell.len()
        + freeze_pane.active_pane.len()
        + freeze_pane.state.len()
}

fn estimate_hyperlink_projection_bytes(hyperlink: &crate::types::HyperlinkProjection) -> usize {
    std::mem::size_of::<crate::types::HyperlinkProjection>()
        + hyperlink.url.len()
        + hyperlink
            .tooltip
            .as_ref()
            .map(String::len)
            .unwrap_or_default()
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
