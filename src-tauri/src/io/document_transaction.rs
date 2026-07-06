use crate::error::AppError;
use crate::io::document_model::{
    DocumentMementoSide, DocumentOperationResult, SpreadsheetDocument,
};
use crate::ops::AppliedOperation;

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

        if self.operation.impact().is_structure_change()
            && let Err(error) = self.document.validate_persisted_projection_consistency()
        {
            self.rollback_after_failure(&error)?;
            return Err(error);
        }

        if let Err(error) = self.document.validate_projection_consistency() {
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
}
