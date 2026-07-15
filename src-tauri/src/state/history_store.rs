use crate::io::document_memento::DocumentMemento;
use crate::state::state::HistoryStatus;

pub(crate) const MAX_HISTORY_ENTRIES: usize = 100;
pub(crate) const MAX_HISTORY_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_SINGLE_HISTORY_ENTRY_BYTES: usize = MAX_HISTORY_BYTES / 2;

pub(crate) struct HistoryEntry {
    pub(crate) memento: DocumentMemento,
    pub(crate) estimated_bytes: usize,
}

impl HistoryEntry {
    pub(crate) fn new(memento: DocumentMemento) -> Self {
        let estimated_bytes = memento.estimated_bytes().max(1);
        Self {
            memento,
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
    truncated_reason: Option<String>,
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
            self.truncated_reason = Some(format!(
                "The last operation was too large to keep in undo history ({} bytes, limit {} bytes).",
                entry.estimated_bytes, MAX_SINGLE_HISTORY_ENTRY_BYTES
            ));
        } else {
            self.push_undo(entry);
        }
        self.clear_redo();
    }

    pub(crate) fn clear_all(&mut self) {
        self.clear_undo();
        self.clear_redo();
        self.truncated_reason = None;
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

    pub(crate) fn peek_undo(&self) -> Option<&HistoryEntry> {
        self.undo_stack.last()
    }

    pub(crate) fn pop_redo(&mut self) -> Option<HistoryEntry> {
        let entry = self.redo_stack.pop()?;
        self.redo_estimated_bytes = self
            .redo_estimated_bytes
            .saturating_sub(entry.estimated_bytes);
        Some(entry)
    }

    pub(crate) fn peek_redo(&self) -> Option<&HistoryEntry> {
        self.redo_stack.last()
    }

    pub(crate) fn push_redo(&mut self, entry: HistoryEntry) {
        self.redo_estimated_bytes += entry.estimated_bytes;
        self.redo_stack.push(entry);
        if evict_oldest_until_bounded(&mut self.redo_stack, &mut self.redo_estimated_bytes) > 0 {
            self.truncated_reason =
                Some("Old redo entries were discarded to keep history under memory budget.".into());
        }
    }

    pub(crate) fn push_undo(&mut self, entry: HistoryEntry) {
        self.undo_estimated_bytes += entry.estimated_bytes;
        self.undo_stack.push(entry);
        if evict_oldest_until_bounded(&mut self.undo_stack, &mut self.undo_estimated_bytes) > 0 {
            self.truncated_reason =
                Some("Old undo entries were discarded to keep history under memory budget.".into());
        }
    }

    pub(crate) fn status(&self) -> HistoryStatus {
        HistoryStatus {
            is_truncated: self.truncated_reason.is_some(),
            reason: self.truncated_reason.clone(),
            undo_entries: self.undo_stack.len(),
            redo_entries: self.redo_stack.len(),
            undo_estimated_bytes: self.undo_estimated_bytes,
            redo_estimated_bytes: self.redo_estimated_bytes,
            max_history_bytes: MAX_HISTORY_BYTES,
            max_single_entry_bytes: MAX_SINGLE_HISTORY_ENTRY_BYTES,
        }
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(self.undo_estimated_bytes)
            .saturating_add(self.redo_estimated_bytes)
            .saturating_add(self.truncated_reason.as_ref().map_or(0, String::capacity))
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

fn evict_oldest_until_bounded(stack: &mut Vec<HistoryEntry>, estimated_bytes: &mut usize) -> usize {
    let mut evicted_count = 0;
    while stack.len() > MAX_HISTORY_ENTRIES || *estimated_bytes > MAX_HISTORY_BYTES {
        let evicted = stack.remove(0);
        *estimated_bytes = estimated_bytes.saturating_sub(evicted.estimated_bytes);
        evicted_count += 1;
        if stack.is_empty() {
            break;
        }
    }
    evicted_count
}
