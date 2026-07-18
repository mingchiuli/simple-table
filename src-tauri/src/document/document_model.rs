use crate::document::backing::document_body::BodySheetShape;
use crate::document::backing::document_body::{BodyRestoreAction, SpreadsheetDocumentBody};
use crate::document::document_memento::{
    CellMemento, ColumnStructureMemento, DocumentMemento, DocumentMementoSide,
    FileStructureMemento, LayoutMemento, ProjectionSheetSnapshot, RichProjectionMemento,
    RowStructureMemento, SheetShapeMemento, SheetTailMemento, StructureMemento,
    protected_rich_cell_positions,
};
use crate::document::document_memento_budget;
use crate::document::document_patches::{CurrentStructureShape, restore_structure_changes};
use crate::document::document_restore::{DocumentRestoreChange, DocumentRestoreResult};
use crate::document::document_save::SpreadsheetDocumentSaveSnapshot;
use crate::document::document_transaction::DocumentTransaction;
use crate::document::formula_coordinator::{FormulaCoordinator, FormulaWorkLimits};
use crate::document::region_metadata_index::RegionMetadataIndex;
use crate::document_data::{DocumentData, DocumentSheet};
use crate::domain::{AppliedOperation, DocumentCellChange, ResolvedCellEdit};
use crate::error::AppError;
use crate::formula::cell_ref::FormulaCellRef;
use crate::types::FormulaStatus;
use crate::types::{CellValue, SheetCapabilities, WorkbookCapabilities};
use std::collections::{HashMap, HashSet};
#[cfg(test)]
use umya_spreadsheet::Workbook;

#[derive(Debug, Clone)]
pub struct DocumentOperationResult {
    pub cell_changes: Vec<DocumentCellChange>,
}

/// Canonical spreadsheet document.
///
/// The physical backing preserves format-specific metadata while `DocumentData` is
/// the projection used by editing, formula calculation, search, and dirty hashing.
pub struct SpreadsheetDocument {
    projection: DocumentData,
    body: SpreadsheetDocumentBody,
    cached_capabilities: WorkbookCapabilities,
    formulas: FormulaCoordinator,
    region_metadata: RegionMetadataIndex,
    transaction_failure: Option<String>,
    #[cfg(test)]
    injected_restore_failures: usize,
    #[cfg(test)]
    injected_post_patch_restore_failures: usize,
}

impl SpreadsheetDocument {
    pub fn new(projection: DocumentData) -> Self {
        let body = SpreadsheetDocumentBody::from_projection(&projection, None);
        Self::from_backing(projection, body)
    }

    pub(crate) fn from_backing(
        mut projection: DocumentData,
        body: SpreadsheetDocumentBody,
    ) -> Self {
        let formulas = FormulaCoordinator::new(&mut projection);
        let formula_structure_limitations = formulas.structure_formula_limitations();
        let cached_capabilities = body.capabilities(&formula_structure_limitations);
        let region_metadata = RegionMetadataIndex::from_file_data(&projection);

        Self {
            projection,
            body,
            cached_capabilities,
            formulas,
            region_metadata,
            transaction_failure: None,
            #[cfg(test)]
            injected_restore_failures: 0,
            #[cfg(test)]
            injected_post_patch_restore_failures: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_workbook(projection: DocumentData, workbook: Option<Workbook>) -> Self {
        let body = SpreadsheetDocumentBody::from_projection(&projection, workbook);
        Self::from_backing(projection, body)
    }

    pub fn projection(&self) -> &DocumentData {
        &self.projection
    }

    pub(in crate::document) fn sheet_count(&self) -> usize {
        self.projection.sheets.len()
    }

    pub fn update_identity(&mut self, path: String, file_name: String) {
        self.projection.path = path;
        self.projection.file_name = file_name;
    }

    pub fn formula_status(&self) -> FormulaStatus {
        self.formulas.status()
    }

    pub fn estimated_runtime_bytes(&self) -> usize {
        self.body
            .estimated_bytes()
            .saturating_add(self.formulas.estimated_bytes(&self.projection))
            .saturating_add(self.region_metadata.estimated_bytes())
            .saturating_add(
                self.transaction_failure
                    .as_ref()
                    .map_or(0, String::capacity),
            )
    }

    pub fn region_metadata(
        &self,
        region: &crate::types::SheetRegion,
    ) -> crate::types::SheetRegionMetadata {
        self.region_metadata.project(&self.projection, region)
    }

    pub fn capabilities(&self) -> WorkbookCapabilities {
        let mut capabilities = self.cached_capabilities.clone();
        if let Some(reason) = &self.transaction_failure {
            capabilities.save.can_native_save = false;
            push_unique_reason(&mut capabilities.save.blocked_save_reasons, reason);
            capabilities.structure.can_insert_delete_sheets = false;
            push_unique_reason(
                &mut capabilities.structure.blocked_sheet_structure_reasons,
                reason,
            );
            push_unique_reason(
                &mut capabilities.structure.blocked_structure_reasons,
                reason,
            );
            push_unique_reason(
                &mut capabilities.save.detected_features,
                "failed editor transaction",
            );
            for sheet_capabilities in &mut capabilities.sheets {
                disable_sheet_capabilities(sheet_capabilities, reason);
            }
        }
        capabilities
    }

    pub(crate) fn unsupported_operation_features(
        &mut self,
        operation: &AppliedOperation,
    ) -> Vec<String> {
        self.body.unsupported_operation_features(
            operation,
            &self.formulas.structure_formula_limitations(),
        )
    }

    #[cfg(test)]
    pub fn is_excel_backed(&self) -> bool {
        self.body.is_excel_backed()
    }

    pub fn is_csv_backed(&self) -> bool {
        self.body.is_csv_backed()
    }

    pub fn save_snapshot_for_target(
        &self,
        target_path_or_name: &str,
    ) -> Result<SpreadsheetDocumentSaveSnapshot, AppError> {
        let body = self.body.save_snapshot();
        if self
            .body
            .can_generate_without_projection(target_path_or_name)
        {
            self.validate_persisted_projection_consistency()?;
            Ok(SpreadsheetDocumentSaveSnapshot::validated_native_workbook(
                body,
                self.transaction_failure.clone(),
            ))
        } else {
            Ok(SpreadsheetDocumentSaveSnapshot::projection(
                self.projection.clone(),
                body,
                self.transaction_failure.clone(),
            ))
        }
    }

    #[cfg(test)]
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
        DocumentMemento::new(before, after)
    }

    pub fn capture_memento_side(&mut self, operation: &AppliedOperation) -> DocumentMementoSide {
        match operation {
            AppliedOperation::SetCell {
                sheet_index,
                row,
                col,
                ..
            } => DocumentMementoSide::Cells(self.cell_memento(
                [FormulaCellRef {
                    sheet_index: *sheet_index,
                    row: *row,
                    col: *col,
                }],
                operation_may_change_formula_capabilities(operation),
            )),
            AppliedOperation::SetCells { changes } => {
                DocumentMementoSide::Cells(self.cell_batch_memento(
                    changes,
                    operation_may_change_formula_capabilities(operation),
                ))
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

    pub(crate) fn estimate_memento_side_bytes(&mut self, operation: &AppliedOperation) -> usize {
        document_memento_budget::estimate_memento_side_bytes(
            &self.projection,
            &self.body,
            &mut self.formulas,
            operation,
        )
    }

    pub(crate) fn validate_formula_work(
        &self,
        operation: &AppliedOperation,
        limits: FormulaWorkLimits,
    ) -> Result<(), AppError> {
        self.formulas
            .validate_recalculation_work(operation, &self.projection, limits)
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

    fn fail_restore_if_injected(&mut self) -> Result<(), AppError> {
        #[cfg(test)]
        if self.injected_restore_failures > 0 {
            self.injected_restore_failures -= 1;
            return Err(AppError::WorkbookPatchFailed(
                "injected history restore failure".to_string(),
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn inject_restore_failures(&mut self, count: usize) {
        self.injected_restore_failures = count;
    }

    fn fail_post_patch_restore_if_injected(&mut self) -> Result<(), AppError> {
        #[cfg(test)]
        if self.injected_post_patch_restore_failures > 0 {
            self.injected_post_patch_restore_failures -= 1;
            return Err(AppError::WorkbookPatchFailed(
                "injected post-patch history restore failure".to_string(),
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn inject_post_patch_restore_failures(&mut self, count: usize) {
        self.injected_post_patch_restore_failures = count;
    }

    fn cell_memento(
        &self,
        changed_cells: impl IntoIterator<Item = FormulaCellRef>,
        formula_capabilities_may_change: bool,
    ) -> CellMemento {
        let changed_cells: Vec<FormulaCellRef> = changed_cells.into_iter().collect();
        let formula_cells = self
            .formulas
            .impacted_cells_for_memento(changed_cells.iter().copied(), &self.projection);
        self.cell_positions_memento(
            changed_cells
                .into_iter()
                .chain(formula_cells)
                .map(|cell| (cell.sheet_index, cell.row, cell.col)),
            formula_capabilities_may_change,
        )
    }

    fn cell_batch_memento(
        &self,
        changes: &[ResolvedCellEdit],
        formula_capabilities_may_change: bool,
    ) -> CellMemento {
        self.cell_memento(
            changes.iter().map(|change| FormulaCellRef {
                sheet_index: change.sheet_index,
                row: change.row,
                col: change.col,
            }),
            formula_capabilities_may_change,
        )
    }

    fn cell_positions_memento(
        &self,
        positions_to_capture: impl IntoIterator<Item = (usize, usize, usize)>,
        formula_capabilities_may_change: bool,
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
                DocumentCellChange::new(
                    sheet_index,
                    row,
                    col,
                    self.projection_cell(sheet_index, row, col),
                )
            })
            .collect();

        CellMemento::new(cells, sheet_shapes, formula_capabilities_may_change)
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

        LayoutMemento::new(sheet_index, column_widths, row_heights)
    }

    fn structure_memento(&mut self, operation: &AppliedOperation) -> StructureMemento {
        let formula_sheet_indexes = self
            .formulas
            .structure_memento_sheet_indexes(&self.projection, operation);
        let body = self
            .body
            .capture_structure_memento(operation, formula_sheet_indexes);
        let projection = self.projection_structure_memento(operation);

        StructureMemento::new(projection, body)
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

        self.fail_restore_if_injected()?;
        self.patch_workbook_formula_changes(&memento.cells)?;
        self.restore_cell_shapes(&memento.sheet_shapes);
        self.patch_workbook_cell_shapes(&memento.sheet_shapes)?;
        let formula_changes = self.formulas.rebuild(&mut self.projection);
        if !formula_changes.is_empty() {
            self.patch_workbook_formula_changes(&formula_changes)?;
        }
        if memento.formula_capabilities_may_change {
            self.refresh_capabilities();
        }
        self.fail_post_patch_restore_if_injected()?;
        self.validate_projection_sheets(cell_memento_sheet_indexes(memento))?;
        let mut changes = Vec::new();
        let mut cell_changes = memento.cells.clone();
        for change in formula_changes {
            push_sheet_cell_change_if_missing(&mut cell_changes, change);
        }
        if !cell_changes.is_empty() {
            changes.push(DocumentRestoreChange::Cells(cell_changes));
        }
        changes.extend(shape_restore_changes(&memento.sheet_shapes));
        Ok(DocumentRestoreResult { changes })
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
            return Ok(DocumentRestoreResult::default());
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

        self.fail_restore_if_injected()?;
        self.patch_workbook_layout(memento)?;
        self.fail_post_patch_restore_if_injected()?;
        self.validate_projection_sheets([memento.sheet_index])?;
        Ok(DocumentRestoreResult {
            changes: vec![DocumentRestoreChange::Layout {
                sheet_index: memento.sheet_index,
                column_widths: memento.column_widths.clone(),
                row_heights: memento.row_heights.clone(),
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
        self.fail_restore_if_injected()?;
        self.refresh_capabilities();
        let formula_changes = self.formulas.rebuild(&mut self.projection);
        if !formula_changes.is_empty() {
            self.patch_workbook_formula_changes(&formula_changes)?;
        }
        self.fail_post_patch_restore_if_injected()?;
        self.validate_persisted_projection_consistency()?;
        self.validate_projection_consistency()?;
        let mut changes =
            restore_structure_changes(&current_shape, &memento.projection, &self.projection);
        if changes.is_empty() {
            changes.push(DocumentRestoreChange::ResyncRequired {
                reason: "structure restore changed workbook projection".to_string(),
            });
        }
        if !formula_changes.is_empty() {
            changes.push(DocumentRestoreChange::Cells(formula_changes));
        }
        self.refresh_region_metadata_index();
        Ok(DocumentRestoreResult { changes })
    }

    pub(in crate::document) fn recalculate_after_operation(
        &mut self,
        operation: &AppliedOperation,
    ) -> Vec<DocumentCellChange> {
        let changes = self
            .formulas
            .recalculate_after_operation(operation, &mut self.projection);
        if operation_may_change_formula_capabilities(operation) {
            self.refresh_capabilities();
        }
        changes
    }

    pub(in crate::document) fn patch_workbook_after_operation(
        &mut self,
        operation: &AppliedOperation,
        cell_changes: &[DocumentCellChange],
    ) -> Result<(), AppError> {
        self.body
            .patch_after_operation(&mut self.projection, operation, cell_changes)
            .map_err(|error| AppError::WorkbookPatchFailed(error.to_string()))?;
        if operation_may_change_formula_capabilities(operation) {
            self.refresh_capabilities();
        }
        Ok(())
    }

    pub(in crate::document) fn apply_operation_to_body_and_projection(
        &mut self,
        operation: &AppliedOperation,
    ) -> Result<(), AppError> {
        if let Some(result) = self.body.apply_structure_operation(
            &mut self.projection,
            operation,
            self.formulas.ast_service_mut(),
        )? {
            self.formulas
                .set_pending_structure_diagnostics(result.diagnostics);
            self.refresh_capabilities();
            self.validate_persisted_projection_consistency()?;
            return Ok(());
        }

        if !operation
            .projection_mutation()
            .execute_cells_and_layout(&mut self.projection)
        {
            operation
                .projection_mutation()
                .execute(&mut self.projection);
        }
        Ok(())
    }

    pub(in crate::document) fn patch_workbook_formula_changes(
        &mut self,
        cell_changes: &[DocumentCellChange],
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
        self.cached_capabilities = self
            .body
            .capabilities(&self.formulas.structure_formula_limitations());
    }

    pub(in crate::document) fn validate_projection_consistency(&self) -> Result<(), AppError> {
        self.body.validate_projection_consistency(&self.projection)
    }

    pub(in crate::document) fn validate_projection_sheets(
        &self,
        sheet_indexes: impl IntoIterator<Item = usize>,
    ) -> Result<(), AppError> {
        self.body
            .validate_projection_sheets(&self.projection, sheet_indexes)
    }

    pub(in crate::document) fn validate_persisted_projection_consistency(
        &self,
    ) -> Result<(), AppError> {
        self.body
            .validate_persisted_projection_consistency(&self.projection)
    }

    pub(in crate::document) fn refresh_region_metadata_index(&mut self) {
        self.region_metadata.rebuild(&self.projection);
    }

    pub fn transaction_failure(&self) -> Option<&str> {
        self.transaction_failure.as_deref()
    }

    pub(crate) fn mark_transaction_failed(&mut self, reason: String) {
        self.refresh_region_metadata_index();
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

fn disable_sheet_capabilities(capabilities: &mut SheetCapabilities, reason: &str) {
    capabilities.can_edit_cells = false;
    capabilities.can_resize_rows_columns = false;
    capabilities.can_insert_delete_rows = false;
    capabilities.can_insert_delete_columns = false;
    push_unique_reason(&mut capabilities.blocked_edit_reasons, reason);
    push_unique_reason(&mut capabilities.blocked_resize_reasons, reason);
    push_unique_reason(&mut capabilities.blocked_row_structure_reasons, reason);
    push_unique_reason(&mut capabilities.blocked_column_structure_reasons, reason);
}

fn operation_may_change_formula_capabilities(operation: &AppliedOperation) -> bool {
    match operation {
        AppliedOperation::SetCell {
            old_value,
            new_value,
            ..
        } => formula_capability_signature(old_value) != formula_capability_signature(new_value),
        AppliedOperation::SetCells { changes } => changes.iter().any(|change| {
            formula_capability_signature(&change.old_value)
                != formula_capability_signature(&change.new_value)
        }),
        AppliedOperation::AddRow { .. }
        | AppliedOperation::DeleteRow { .. }
        | AppliedOperation::AddColumn { .. }
        | AppliedOperation::DeleteColumn { .. }
        | AppliedOperation::SetColumnWidth { .. }
        | AppliedOperation::SetRowHeight { .. }
        | AppliedOperation::AddSheet { .. }
        | AppliedOperation::DeleteSheet { .. } => false,
    }
}

fn formula_capability_signature(value: &CellValue) -> Option<&str> {
    match value {
        CellValue::Formula { formula, .. } => Some(formula.as_str()),
        _ => None,
    }
}

fn restore_projection_structure(file_data: &mut DocumentData, memento: &FileStructureMemento) {
    match memento {
        FileStructureMemento::Empty { sheet_count } => {
            file_data.sheets.truncate(*sheet_count);
        }
        FileStructureMemento::Row(memento) => restore_projection_row(file_data, memento),
        FileStructureMemento::Column(memento) => restore_projection_column(file_data, memento),
        FileStructureMemento::Sheets(memento) => restore_projection_sheet_tail(file_data, memento),
    }
}

fn restore_projection_row(file_data: &mut DocumentData, memento: &RowStructureMemento) {
    let Some(sheet) = file_data.sheets.get_mut(memento.sheet_index) else {
        return;
    };
    if sheet.rows.len() > memento.row_count {
        if memento.row_index < sheet.rows.len() {
            sheet.rows.remove(memento.row_index);
        }
    } else if sheet.rows.len() < memento.row_count {
        let row = memento.row.clone().unwrap_or_default();
        while sheet.rows.len() < memento.row_index && sheet.rows.len() < memento.row_count {
            sheet.rows.push(Vec::new());
        }
        if sheet.rows.len() < memento.row_count {
            sheet
                .rows
                .insert(memento.row_index.min(sheet.rows.len()), row);
        }
    } else if let Some(row) = &memento.row
        && memento.row_index < sheet.rows.len()
    {
        sheet.rows[memento.row_index] = row.clone();
    }

    if sheet.rows.len() < memento.row_count {
        sheet.rows.resize_with(memento.row_count, Vec::new);
    }
    sheet.rows.truncate(memento.row_count);
    sheet.merges = memento.merges.clone();
    sheet.row_heights = memento.row_heights.clone();
    memento.rich.restore_into(&mut sheet.rich);
}

fn restore_projection_column(file_data: &mut DocumentData, memento: &ColumnStructureMemento) {
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
            while row.len() < memento.col_index && row.len() < target_len {
                row.push(CellValue::Null);
            }
            if row.len() < target_len {
                row.insert(
                    memento.col_index.min(row.len()),
                    value.unwrap_or(CellValue::Null),
                );
            }
        } else if let Some(value) = value
            && memento.col_index < row.len()
        {
            row[memento.col_index] = value;
        }
        if row.len() < target_len {
            row.resize(target_len, CellValue::Null);
        }
        row.truncate(target_len);
    }

    sheet.merges = memento.merges.clone();
    sheet.column_widths = memento.column_widths.clone();
    memento.rich.restore_into(&mut sheet.rich);
}

fn restore_projection_sheet_tail(file_data: &mut DocumentData, memento: &SheetTailMemento) {
    file_data.sheets.truncate(memento.truncate_from);

    for snapshot in &memento.sheets {
        if file_data.sheets.len() < snapshot.sheet_index {
            file_data
                .sheets
                .resize_with(snapshot.sheet_index, DocumentSheet::default);
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

fn push_sheet_cell_change_if_missing(
    changes: &mut Vec<DocumentCellChange>,
    change: DocumentCellChange,
) {
    if !changes.iter().any(|existing| {
        existing.sheet_index == change.sheet_index
            && existing.row == change.row
            && existing.col == change.col
    }) {
        changes.push(change);
    }
}

fn cell_memento_sheet_indexes(memento: &CellMemento) -> Vec<usize> {
    let mut sheets = HashSet::new();
    for change in &memento.cells {
        sheets.insert(change.sheet_index);
    }
    for shape in &memento.sheet_shapes {
        sheets.insert(shape.sheet_index);
    }
    sheets.into_iter().collect()
}

fn shape_restore_changes(shapes: &[SheetShapeMemento]) -> Vec<DocumentRestoreChange> {
    let mut sheet_indexes = shapes
        .iter()
        .map(|shape| shape.sheet_index)
        .collect::<Vec<_>>();
    sheet_indexes.sort_unstable();
    sheet_indexes.dedup();
    sheet_indexes
        .into_iter()
        .map(|sheet_index| DocumentRestoreChange::SheetInvalidated { sheet_index })
        .collect()
}

fn ensure_projection_cell_exists(sheet: &mut DocumentSheet, row: usize, col: usize) {
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
