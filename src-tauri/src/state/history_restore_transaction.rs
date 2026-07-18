use crate::document::document_memento::{DocumentMemento, DocumentMementoSide};
use crate::document::document_model::SpreadsheetDocument;
use crate::document::document_restore::DocumentRestoreResult;
use crate::error::AppError;
use crate::state::dirty_tracker::DirtyTracker;
use crate::state::history_store::{HistoryStore, RetiredHistoryEntries};

#[derive(Clone, Copy)]
pub(crate) enum HistoryRestoreDirection {
    Undo,
    Redo,
}

pub(crate) struct HistoryRestoreTransaction<'a> {
    document: &'a mut SpreadsheetDocument,
    history: &'a mut HistoryStore,
    dirty: &'a mut DirtyTracker,
    direction: HistoryRestoreDirection,
}

impl<'a> HistoryRestoreTransaction<'a> {
    pub(crate) fn new(
        document: &'a mut SpreadsheetDocument,
        history: &'a mut HistoryStore,
        dirty: &'a mut DirtyTracker,
        direction: HistoryRestoreDirection,
    ) -> Self {
        Self {
            document,
            history,
            dirty,
            direction,
        }
    }

    pub(crate) fn commit(
        &mut self,
    ) -> Result<Option<(DocumentRestoreResult, RetiredHistoryEntries)>, AppError> {
        let direction = self.direction;
        let entry = match direction {
            HistoryRestoreDirection::Undo => self.history.peek_undo(),
            HistoryRestoreDirection::Redo => self.history.peek_redo(),
        };
        let Some(entry) = entry else {
            return Ok(None);
        };
        let (target, rollback) = restore_sides(&entry.memento, direction);
        let restore = match self.document.restore_memento_side(target) {
            Ok(restore) => restore,
            Err(error) => {
                rollback_after_failure(self.document, rollback, &error)?;
                return Err(error);
            }
        };

        self.dirty
            .apply_history_restore(target, rollback, self.document.projection());
        let retired = self.move_history_entry();
        Ok(Some((restore, retired)))
    }

    fn move_history_entry(&mut self) -> RetiredHistoryEntries {
        match self.direction {
            HistoryRestoreDirection::Undo => {
                let entry = self
                    .history
                    .pop_undo()
                    .expect("undo entry exists until restore transaction commits");
                self.history.push_redo(entry)
            }
            HistoryRestoreDirection::Redo => {
                let entry = self
                    .history
                    .pop_redo()
                    .expect("redo entry exists until restore transaction commits");
                self.history.push_undo(entry)
            }
        }
    }
}

fn rollback_after_failure(
    document: &mut SpreadsheetDocument,
    rollback: &DocumentMementoSide,
    restore_error: &AppError,
) -> Result<(), AppError> {
    match document.restore_memento_side(rollback) {
        Ok(_) => Ok(()),
        Err(rollback_error) => {
            let operation_error = restore_error.to_string();
            let rollback_error = rollback_error.to_string();
            document.mark_transaction_failed(format!(
                "history restore failed ({operation_error}) and rollback failed ({rollback_error})"
            ));
            Err(AppError::TransactionRollbackFailed {
                operation_error,
                rollback_error,
            })
        }
    }
}

fn restore_sides(
    memento: &DocumentMemento,
    direction: HistoryRestoreDirection,
) -> (&DocumentMementoSide, &DocumentMementoSide) {
    match direction {
        HistoryRestoreDirection::Undo => (&memento.before, &memento.after),
        HistoryRestoreDirection::Redo => (&memento.after, &memento.before),
    }
}
