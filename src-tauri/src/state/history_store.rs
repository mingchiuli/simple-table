use crate::io::document_model::DocumentMemento;
use crate::types::AppliedOperationResult;

pub(crate) const MAX_HISTORY_ENTRIES: usize = 100;
pub(crate) const MAX_HISTORY_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_SINGLE_HISTORY_ENTRY_BYTES: usize = MAX_HISTORY_BYTES / 2;

pub(crate) struct HistoryEntry {
    pub(crate) memento: DocumentMemento,
    pub(crate) operation: AppliedOperationResult,
    pub(crate) estimated_bytes: usize,
}

impl HistoryEntry {
    pub(crate) fn new(memento: DocumentMemento, operation: AppliedOperationResult) -> Self {
        let estimated_bytes = memento.estimated_bytes().max(1);
        Self {
            memento,
            operation,
            estimated_bytes,
        }
    }
}

#[derive(Default)]
pub(crate) struct HistoryStore {
    undo_stack: Vec<HistoryEntry>,
    redo_stack: Vec<HistoryEntry>,
    undo_estimated_bytes: usize,
    redo_estimated_bytes: usize,
}

impl HistoryStore {
    pub(crate) fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub(crate) fn record(&mut self, entry: HistoryEntry) {
        if entry.estimated_bytes > MAX_SINGLE_HISTORY_ENTRY_BYTES {
            self.clear_undo();
        } else {
            self.push_undo(entry);
        }
        self.clear_redo();
    }

    pub(crate) fn clear_all(&mut self) {
        self.clear_undo();
        self.clear_redo();
    }

    pub(crate) fn clear_undo(&mut self) {
        self.undo_stack.clear();
        self.undo_estimated_bytes = 0;
    }

    pub(crate) fn clear_redo(&mut self) {
        self.redo_stack.clear();
        self.redo_estimated_bytes = 0;
    }

    pub(crate) fn pop_undo(&mut self) -> Option<HistoryEntry> {
        let entry = self.undo_stack.pop()?;
        self.undo_estimated_bytes = self
            .undo_estimated_bytes
            .saturating_sub(entry.estimated_bytes);
        Some(entry)
    }

    pub(crate) fn pop_redo(&mut self) -> Option<HistoryEntry> {
        let entry = self.redo_stack.pop()?;
        self.redo_estimated_bytes = self
            .redo_estimated_bytes
            .saturating_sub(entry.estimated_bytes);
        Some(entry)
    }

    pub(crate) fn push_redo(&mut self, entry: HistoryEntry) {
        self.redo_estimated_bytes += entry.estimated_bytes;
        self.redo_stack.push(entry);
        evict_oldest_until_bounded(&mut self.redo_stack, &mut self.redo_estimated_bytes);
    }

    pub(crate) fn push_undo(&mut self, entry: HistoryEntry) {
        self.undo_estimated_bytes += entry.estimated_bytes;
        self.undo_stack.push(entry);
        evict_oldest_until_bounded(&mut self.undo_stack, &mut self.undo_estimated_bytes);
    }

    #[cfg(test)]
    pub(crate) fn undo_len(&self) -> usize {
        self.undo_stack.len()
    }

    #[cfg(test)]
    pub(crate) fn undo_estimated_bytes(&self) -> usize {
        self.undo_estimated_bytes
    }
}

fn evict_oldest_until_bounded(stack: &mut Vec<HistoryEntry>, estimated_bytes: &mut usize) {
    while stack.len() > MAX_HISTORY_ENTRIES || *estimated_bytes > MAX_HISTORY_BYTES {
        let evicted = stack.remove(0);
        *estimated_bytes = estimated_bytes.saturating_sub(evicted.estimated_bytes);
        if stack.is_empty() {
            break;
        }
    }
}
