use crate::state::search_index::{
    MAX_RESIDENT_SEARCH_INDEXES, SearchCellSnapshotChange, SearchCellText, SearchIndexStamp,
    SearchIndexStore, SearchSheetIndex, SearchSheetSnapshot, SearchSheetSource, SearchWriterHandle,
};
use crate::types::FileData;
use std::sync::Arc;

#[derive(Default)]
pub struct SearchSession {
    index: SearchIndexStore,
    snapshots: Vec<Arc<SearchSheetSnapshot>>,
}

impl SearchSession {
    pub fn from_file_data(file_data: &FileData, revision: u64) -> Self {
        let mut session = Self::default();
        session.replace_snapshots(file_data, revision);
        session
    }

    pub fn replace_snapshots(&mut self, file_data: &FileData, revision: u64) {
        self.snapshots = file_data
            .sheets
            .iter()
            .take(MAX_RESIDENT_SEARCH_INDEXES)
            .map(|sheet| SearchSheetSnapshot::from_sheet(sheet, revision))
            .collect();
    }

    pub fn update_snapshots(&mut self, revision: u64, changes: Vec<SearchCellSnapshotChange>) {
        let mut by_sheet = std::collections::BTreeMap::<usize, Vec<_>>::new();
        for change in changes {
            by_sheet.entry(change.sheet_index).or_default().push(change);
        }
        for (sheet_index, changes) in by_sheet {
            let Some(parent) = self.snapshots.get(sheet_index).cloned() else {
                continue;
            };
            if parent.revision() > revision {
                continue;
            }
            self.snapshots[sheet_index] =
                SearchSheetSnapshot::with_changes(parent, changes, revision);
        }
    }

    pub fn sheet_source(&self, sheet_index: usize) -> Option<SearchSheetSource> {
        if let Some(index) = self.index.fresh_sheet_index(sheet_index) {
            return Some(SearchSheetSource::Indexed(index));
        }
        self.snapshots
            .get(sheet_index)
            .cloned()
            .map(SearchSheetSource::Snapshot)
    }

    pub fn sheet_snapshot(&self, sheet_index: usize) -> Option<Arc<SearchSheetSnapshot>> {
        self.snapshots.get(sheet_index).cloned()
    }

    pub fn compact_snapshot(
        &mut self,
        sheet_index: usize,
        revision: u64,
        cells: Arc<[SearchCellText]>,
    ) {
        if let Some(snapshot) = self.snapshots.get_mut(sheet_index)
            && snapshot.revision() <= revision
        {
            *snapshot = SearchSheetSnapshot::from_cells(cells, revision);
        }
    }

    pub fn sheet_stamp(&self, document_id: u64, sheet_index: usize) -> SearchIndexStamp {
        self.index.sheet_stamp(document_id, sheet_index)
    }

    pub fn install_sheet_index(
        &mut self,
        document_id: u64,
        sheet_index: usize,
        sheet_count: usize,
        stamp: SearchIndexStamp,
        index: Option<SearchSheetIndex>,
    ) {
        self.index
            .install_sheet_index(document_id, sheet_index, stamp, index);
        self.index.truncate(sheet_count);
    }

    pub fn mark_all_stale(&mut self, document_id: u64) -> SearchIndexStamp {
        self.index.mark_stale(document_id)
    }

    pub fn mark_sheets_stale(&mut self, sheet_indexes: impl IntoIterator<Item = usize>) {
        for sheet_index in sheet_indexes {
            self.index.mark_sheet_stale(sheet_index);
        }
    }

    pub fn mark_sheet_fresh(
        &mut self,
        document_id: u64,
        sheet_index: usize,
        stamp: SearchIndexStamp,
    ) {
        self.index.mark_sheet_fresh(document_id, sheet_index, stamp);
    }

    pub fn writer_handle(
        &self,
        document_id: u64,
        sheet_index: usize,
        stamp: SearchIndexStamp,
    ) -> Option<SearchWriterHandle> {
        self.index.writer_handle(document_id, sheet_index, stamp)
    }
}
