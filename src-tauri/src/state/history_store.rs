use crate::document::document_memento::DocumentMemento;
use std::collections::VecDeque;

pub(crate) const MAX_HISTORY_ENTRIES: usize = 100;
pub(crate) const MAX_HISTORY_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_SINGLE_HISTORY_ENTRY_BYTES: usize = MAX_HISTORY_BYTES / 2;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct HistoryStatus {
    pub is_truncated: bool,
    pub reason: Option<String>,
    pub undo_entries: usize,
    pub redo_entries: usize,
    pub undo_estimated_bytes: usize,
    pub redo_estimated_bytes: usize,
    pub max_history_bytes: usize,
    pub max_single_entry_bytes: usize,
}

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
    undo_stack: VecDeque<HistoryEntry>,
    redo_stack: VecDeque<HistoryEntry>,
    undo_estimated_bytes: usize,
    redo_estimated_bytes: usize,
    truncated_reason: Option<String>,
}

#[derive(Default)]
pub(crate) struct RetiredHistoryEntries {
    entries: Vec<HistoryEntry>,
}

impl RetiredHistoryEntries {
    pub(crate) fn append(&mut self, mut other: Self) {
        self.entries.append(&mut other.entries);
    }

    fn push(&mut self, entry: HistoryEntry) {
        self.entries.push(entry);
    }

    fn extend(&mut self, entries: impl IntoIterator<Item = HistoryEntry>) {
        self.entries.extend(entries);
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

impl HistoryStore {
    pub(crate) fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub(crate) fn record(&mut self, entry: HistoryEntry) -> RetiredHistoryEntries {
        let mut retired = RetiredHistoryEntries::default();
        if entry.estimated_bytes > MAX_SINGLE_HISTORY_ENTRY_BYTES {
            let entry_bytes = entry.estimated_bytes;
            retired.append(self.clear_undo());
            retired.push(entry);
            self.truncated_reason = Some(format!(
                "The last operation was too large to keep in undo history ({} bytes, limit {} bytes).",
                entry_bytes, MAX_SINGLE_HISTORY_ENTRY_BYTES
            ));
        } else {
            retired.append(self.push_undo(entry));
        }
        retired.append(self.clear_redo());
        retired
    }

    pub(crate) fn clear_all(&mut self) -> RetiredHistoryEntries {
        let mut retired = self.clear_undo();
        retired.append(self.clear_redo());
        self.truncated_reason = None;
        retired
    }

    pub(crate) fn clear_undo(&mut self) -> RetiredHistoryEntries {
        let mut retired = RetiredHistoryEntries::default();
        retired.extend(std::mem::take(&mut self.undo_stack));
        self.undo_estimated_bytes = 0;
        retired
    }

    pub(crate) fn clear_redo(&mut self) -> RetiredHistoryEntries {
        let mut retired = RetiredHistoryEntries::default();
        retired.extend(std::mem::take(&mut self.redo_stack));
        self.redo_estimated_bytes = 0;
        retired
    }

    pub(crate) fn pop_undo(&mut self) -> Option<HistoryEntry> {
        let entry = self.undo_stack.pop_back()?;
        self.undo_estimated_bytes = self
            .undo_estimated_bytes
            .saturating_sub(entry.estimated_bytes);
        Some(entry)
    }

    pub(crate) fn peek_undo(&self) -> Option<&HistoryEntry> {
        self.undo_stack.back()
    }

    pub(crate) fn pop_redo(&mut self) -> Option<HistoryEntry> {
        let entry = self.redo_stack.pop_back()?;
        self.redo_estimated_bytes = self
            .redo_estimated_bytes
            .saturating_sub(entry.estimated_bytes);
        Some(entry)
    }

    pub(crate) fn peek_redo(&self) -> Option<&HistoryEntry> {
        self.redo_stack.back()
    }

    pub(crate) fn push_redo(&mut self, entry: HistoryEntry) -> RetiredHistoryEntries {
        self.redo_estimated_bytes += entry.estimated_bytes;
        self.redo_stack.push_back(entry);
        let retired =
            evict_oldest_until_bounded(&mut self.redo_stack, &mut self.redo_estimated_bytes);
        if !retired.entries.is_empty() {
            self.truncated_reason =
                Some("Old redo entries were discarded to keep history under memory budget.".into());
        }
        retired
    }

    pub(crate) fn push_undo(&mut self, entry: HistoryEntry) -> RetiredHistoryEntries {
        self.undo_estimated_bytes += entry.estimated_bytes;
        self.undo_stack.push_back(entry);
        let retired =
            evict_oldest_until_bounded(&mut self.undo_stack, &mut self.undo_estimated_bytes);
        if !retired.entries.is_empty() {
            self.truncated_reason =
                Some("Old undo entries were discarded to keep history under memory budget.".into());
        }
        retired
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

fn evict_oldest_until_bounded(
    stack: &mut VecDeque<HistoryEntry>,
    estimated_bytes: &mut usize,
) -> RetiredHistoryEntries {
    let mut retired = RetiredHistoryEntries::default();
    while stack.len() > MAX_HISTORY_ENTRIES || *estimated_bytes > MAX_HISTORY_BYTES {
        let Some(evicted) = stack.pop_front() else {
            break;
        };
        *estimated_bytes = estimated_bytes.saturating_sub(evicted.estimated_bytes);
        retired.push(evicted);
    }
    retired
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::document_memento::{DocumentMementoSide, LayoutMemento};
    use std::collections::HashMap;

    fn entry(estimated_bytes: usize) -> HistoryEntry {
        let side =
            || DocumentMementoSide::Layout(LayoutMemento::new(0, HashMap::new(), HashMap::new()));
        HistoryEntry {
            memento: DocumentMemento::new(side(), side()),
            estimated_bytes,
        }
    }

    #[test]
    fn recording_after_undo_returns_the_cleared_redo_entry() {
        let mut history = HistoryStore::default();
        assert_eq!(history.record(entry(1)).len(), 0);
        let previous = history.pop_undo().expect("undo entry");
        assert_eq!(history.push_redo(previous).len(), 0);

        let retired = history.record(entry(1));

        assert_eq!(retired.len(), 1);
        assert!(!history.can_redo());
    }

    #[test]
    fn capacity_eviction_returns_the_oldest_entry() {
        let mut history = HistoryStore::default();
        for _ in 0..MAX_HISTORY_ENTRIES {
            assert_eq!(history.push_undo(entry(1)).len(), 0);
        }

        let retired = history.push_undo(entry(1));

        assert_eq!(retired.len(), 1);
        assert_eq!(history.undo_stack.len(), MAX_HISTORY_ENTRIES);
    }
}
