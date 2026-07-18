use crate::document::document_memento::DocumentMementoSide;
use crate::domain::{AppliedOperation, DocumentCellChange};
use crate::state::content_hash::{ContentHash, IncrementalContentFingerprint};
use crate::types::FileData;

pub struct DirtyTracker {
    current: IncrementalContentFingerprint,
    saved_content_hash: ContentHash,
}

impl DirtyTracker {
    pub fn new(file_data: &FileData) -> Self {
        let current = IncrementalContentFingerprint::from_file_data(file_data);
        Self {
            saved_content_hash: current.hash(),
            current,
        }
    }

    #[cfg(test)]
    pub fn current_hash(&self) -> ContentHash {
        self.current.hash()
    }

    pub fn is_dirty(&self) -> bool {
        self.current.hash() != self.saved_content_hash
    }

    pub fn replace_current(&mut self, file_data: &FileData) {
        self.current = IncrementalContentFingerprint::from_file_data(file_data);
    }

    pub fn apply_operation(
        &mut self,
        operation: &AppliedOperation,
        formula_changes: &[DocumentCellChange],
        file_data: &FileData,
    ) {
        self.current
            .apply_operation(operation, formula_changes, file_data);
    }

    pub fn apply_history_restore(
        &mut self,
        target: &DocumentMementoSide,
        rollback: &DocumentMementoSide,
        file_data: &FileData,
    ) {
        self.current
            .apply_history_restore(target, rollback, file_data);
    }

    pub fn mark_saved(&mut self) {
        self.saved_content_hash = self.current.hash();
    }
}
