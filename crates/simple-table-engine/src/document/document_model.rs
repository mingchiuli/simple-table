use crate::document::backing::document_body::{BodyRestoreAction, SpreadsheetDocumentBody};
use crate::document::backing::workbook_patch::WorkbookSheetShape;
use crate::document::capabilities::{SheetCapabilities, WorkbookCapabilities};
use crate::document::data::{DocumentData, DocumentSheet};
use crate::document::document_memento::{
    CellMemento, ColumnStructureMemento, DocumentMemento, DocumentMementoSide,
    FileStructureMemento, ImageMemento, LayoutMemento, ProjectionSheetSnapshot,
    RichProjectionMemento, RowStructureMemento, SheetShapeMemento, SheetTailMemento,
    StructureMemento, protected_rich_cell_positions,
};
use crate::document::document_memento_budget;
use crate::document::document_patches::{CurrentStructureShape, restore_structure_changes};
use crate::document::document_restore::{DocumentRestoreChange, DocumentRestoreResult};
use crate::document::document_save::SpreadsheetDocumentSaveSnapshot;
use crate::document::formula_coordinator::{FormulaCoordinator, FormulaWorkLimits};
use crate::document::region_metadata_index::{
    DocumentRegion, DocumentRegionMetadata, RegionMetadataIndex,
};
use crate::document::resource_estimator::estimate_document_metadata_bytes;
use crate::domain::{AppliedOperation, CellValue, DocumentCellChange, ResolvedCellEdit};
use crate::error::AppError;
use crate::formula::cell_ref::FormulaCellRef;
use crate::formula::status::FormulaStatus;
use std::collections::{BTreeSet, HashMap, HashSet};
#[cfg(test)]
use umya_spreadsheet::Workbook;

mod memento_ops;

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
        let body = SpreadsheetDocumentBody::from_projection(&projection);
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
        let body = match workbook {
            Some(workbook) => SpreadsheetDocumentBody::from_workbook(
                workbook,
                crate::document::test_support::workbook_backing_port(),
            ),
            None => SpreadsheetDocumentBody::from_projection(&projection),
        };
        Self::from_backing(projection, body)
    }

    pub fn projection(&self) -> &DocumentData {
        &self.projection
    }

    pub(crate) fn image_bytes(
        &self,
        sheet_index: usize,
        image_id: &str,
    ) -> Option<std::sync::Arc<[u8]>> {
        let image = self
            .projection
            .sheets
            .get(sheet_index)?
            .rich
            .images
            .iter()
            .find(|image| image.id == image_id && image.renderable)?;
        self.body.image_bytes(sheet_index, image.z_index)
    }

    fn sheet_count(&self) -> usize {
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
        let retained_workbook_metadata = if self.body.is_excel_backed() {
            estimate_document_metadata_bytes(&self.projection)
        } else {
            0
        };
        self.body
            .estimated_bytes()
            .saturating_add(retained_workbook_metadata)
            .saturating_add(self.formulas.estimated_bytes(&self.projection))
            .saturating_add(self.region_metadata.estimated_bytes())
            .saturating_add(
                self.transaction_failure
                    .as_ref()
                    .map_or(0, String::capacity),
            )
    }

    pub fn region_metadata(&self, region: &DocumentRegion) -> DocumentRegionMetadata {
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
            capabilities.rich.images.can_insert = false;
            capabilities.rich.images.can_move_resize = false;
            capabilities.rich.images.can_delete = false;
            push_unique_reason(&mut capabilities.rich.images.blocked_reasons, reason);
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
        let body = self.body.save_snapshot(&self.projection)?;
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

    pub(crate) fn update_excel_save_baseline(&mut self, bytes: std::sync::Arc<[u8]>) {
        self.body
            .update_excel_save_baseline(bytes, &self.projection);
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
        self.commit_operation(operation, rollback)
    }

    fn commit_operation(
        &mut self,
        operation: &AppliedOperation,
        rollback: &DocumentMementoSide,
    ) -> Result<DocumentOperationResult, AppError> {
        if let Err(error) = self.apply_operation_to_body_and_projection(operation) {
            self.rollback_operation_after_failure(rollback, &error)?;
            return Err(error);
        }

        if let Err(error) = self.patch_workbook_after_operation(operation, &[]) {
            self.rollback_operation_after_failure(rollback, &error)?;
            return Err(error);
        }

        let cell_changes = self.recalculate_after_operation(operation);
        if !cell_changes.is_empty()
            && let Err(error) = self.patch_workbook_formula_changes(&cell_changes)
        {
            self.rollback_operation_after_failure(rollback, &error)?;
            return Err(error);
        }

        if let Err(error) = self.validate_after_operation(operation, &cell_changes) {
            self.rollback_operation_after_failure(rollback, &error)?;
            return Err(error);
        }
        if operation.impact().is_structure_change() {
            self.refresh_region_metadata_index();
        }

        Ok(DocumentOperationResult { cell_changes })
    }

    fn rollback_operation_after_failure(
        &mut self,
        rollback: &DocumentMementoSide,
        operation_error: &AppError,
    ) -> Result<(), AppError> {
        match self.restore_memento_side(rollback) {
            Ok(_) => Ok(()),
            Err(rollback_error) => {
                let operation_error = operation_error.to_string();
                let rollback_error = rollback_error.to_string();
                self.mark_transaction_failed(format!(
                    "operation failed ({operation_error}) and rollback failed ({rollback_error})"
                ));
                Err(AppError::TransactionRollbackFailed {
                    operation_error,
                    rollback_error,
                })
            }
        }
    }

    fn validate_after_operation(
        &self,
        operation: &AppliedOperation,
        cell_changes: &[DocumentCellChange],
    ) -> Result<(), AppError> {
        if operation.impact().is_structure_change() {
            self.validate_persisted_projection_consistency()?;
            return self.validate_projection_consistency();
        }

        self.validate_projection_sheets(touched_sheet_indexes(
            operation,
            cell_changes,
            self.sheet_count(),
        ))
    }

    pub(crate) fn validate_formula_work(
        &self,
        operation: &AppliedOperation,
        limits: FormulaWorkLimits,
    ) -> Result<(), AppError> {
        self.formulas
            .validate_recalculation_work(operation, &self.projection, limits)
    }

    fn recalculate_after_operation(
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

    fn patch_workbook_after_operation(
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

    fn apply_operation_to_body_and_projection(
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

    fn patch_workbook_formula_changes(
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
        let sheet_shapes: Vec<WorkbookSheetShape> = shapes
            .iter()
            .map(|shape| WorkbookSheetShape {
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

    fn validate_projection_consistency(&self) -> Result<(), AppError> {
        self.body.validate_projection_consistency(&self.projection)
    }

    fn validate_projection_sheets(
        &self,
        sheet_indexes: impl IntoIterator<Item = usize>,
    ) -> Result<(), AppError> {
        self.body
            .validate_projection_sheets(&self.projection, sheet_indexes)
    }

    fn validate_persisted_projection_consistency(&self) -> Result<(), AppError> {
        self.body
            .validate_persisted_projection_consistency(&self.projection)
    }

    fn refresh_region_metadata_index(&mut self) {
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

fn touched_sheet_indexes(
    operation: &AppliedOperation,
    cell_changes: &[DocumentCellChange],
    sheet_count: usize,
) -> Vec<usize> {
    let mut sheets = BTreeSet::new();
    match operation {
        AppliedOperation::SetCell { sheet_index, .. }
        | AppliedOperation::SetColumnWidth { sheet_index, .. }
        | AppliedOperation::SetRowHeight { sheet_index, .. } => {
            sheets.insert(*sheet_index);
        }
        AppliedOperation::SortRows(sort) => {
            sheets.insert(sort.sheet_index);
        }
        AppliedOperation::InsertImage { sheet_index, .. }
        | AppliedOperation::UpdateImage { sheet_index, .. }
        | AppliedOperation::DeleteImage { sheet_index, .. } => {
            sheets.insert(*sheet_index);
        }
        AppliedOperation::SetCells { changes } => {
            for change in changes {
                sheets.insert(change.sheet_index);
            }
        }
        AppliedOperation::AddRow { sheet_index, .. }
        | AppliedOperation::DeleteRow { sheet_index, .. }
        | AppliedOperation::AddColumn { sheet_index, .. }
        | AppliedOperation::DeleteColumn { sheet_index, .. }
        | AppliedOperation::AddSheet { sheet_index, .. } => {
            sheets.insert(*sheet_index);
        }
        AppliedOperation::DeleteSheet { sheet_index } => {
            for shifted_sheet_index in (*sheet_index).min(sheet_count)..sheet_count {
                sheets.insert(shifted_sheet_index);
            }
        }
    }
    for change in cell_changes {
        sheets.insert(change.sheet_index);
    }
    sheets.into_iter().collect()
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
        | AppliedOperation::DeleteSheet { .. }
        | AppliedOperation::InsertImage { .. }
        | AppliedOperation::UpdateImage { .. }
        | AppliedOperation::DeleteImage { .. }
        | AppliedOperation::SortRows(_) => false,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_sheet_validation_targets_shifted_remaining_sheets() {
        let indexes =
            touched_sheet_indexes(&AppliedOperation::DeleteSheet { sheet_index: 1 }, &[], 3);

        assert_eq!(indexes, vec![1, 2]);
    }

    #[test]
    fn delete_last_sheet_validation_does_not_target_removed_index() {
        let indexes =
            touched_sheet_indexes(&AppliedOperation::DeleteSheet { sheet_index: 1 }, &[], 1);

        assert!(indexes.is_empty());
    }
}
