use crate::error::AppError;
use crate::io::document_memento::DocumentMementoSide;
use crate::io::document_model::{DocumentOperationResult, SpreadsheetDocument};
use crate::ops::AppliedOperation;
use crate::types::SheetCellChange;
use std::collections::BTreeSet;

pub(crate) struct DocumentTransaction<'a> {
    document: &'a mut SpreadsheetDocument,
    operation: &'a AppliedOperation,
    rollback: &'a DocumentMementoSide,
}

impl<'a> DocumentTransaction<'a> {
    pub(crate) fn new(
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

    pub(crate) fn commit(&mut self) -> Result<DocumentOperationResult, AppError> {
        let result = match self
            .document
            .apply_operation_to_body_and_projection(self.operation)
        {
            Ok(result) => result,
            Err(error) => {
                self.rollback_after_failure(&error)?;
                return Err(error);
            }
        };

        if let Err(error) =
            self.document
                .patch_workbook_after_operation(self.operation, &result, &[])
        {
            self.rollback_after_failure(&error)?;
            return Err(error);
        }

        let cell_changes = self.document.recalculate_after_operation(self.operation);

        if !cell_changes.is_empty()
            && let Err(error) = self.document.patch_workbook_formula_changes(&cell_changes)
        {
            self.rollback_after_failure(&error)?;
            return Err(error);
        }

        if let Err(error) = self.validate_after_commit(&cell_changes) {
            self.rollback_after_failure(&error)?;
            return Err(error);
        }

        Ok(DocumentOperationResult {
            operation: result,
            cell_changes,
        })
    }

    fn rollback_after_failure(&mut self, operation_error: &AppError) -> Result<(), AppError> {
        match self.document.restore_memento_side(self.rollback) {
            Ok(_) => Ok(()),
            Err(rollback_error) => {
                let operation_error = operation_error.to_string();
                let rollback_error = rollback_error.to_string();
                self.document.mark_transaction_failed(format!(
                    "operation failed ({operation_error}) and rollback failed ({rollback_error})"
                ));
                Err(AppError::TransactionRollbackFailed {
                    operation_error,
                    rollback_error,
                })
            }
        }
    }

    fn validate_after_commit(&self, cell_changes: &[SheetCellChange]) -> Result<(), AppError> {
        if self.operation.impact().is_structure_change() {
            self.document.validate_persisted_projection_consistency()?;
            return self.document.validate_projection_consistency();
        }

        self.document
            .validate_projection_sheets(touched_sheet_indexes(
                self.operation,
                cell_changes,
                self.document.sheet_count(),
            ))
    }
}

fn touched_sheet_indexes(
    operation: &AppliedOperation,
    cell_changes: &[SheetCellChange],
    sheet_count: usize,
) -> Vec<usize> {
    let mut sheets = BTreeSet::new();
    match operation {
        AppliedOperation::SetCell { sheet_index, .. }
        | AppliedOperation::SetColumnWidth { sheet_index, .. }
        | AppliedOperation::SetRowHeight { sheet_index, .. } => {
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
