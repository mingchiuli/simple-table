use crate::state::search_index::{
    SearchIndexStamp, SearchIndexStore, SearchSheetIndex, SearchWriterHandle,
};
use crate::types::FileData;
use std::sync::Arc;

#[derive(Default)]
pub struct SearchSession {
    index: SearchIndexStore,
}

impl SearchSession {
    pub fn from_file_data(_file_data: &FileData, _revision: u64) -> Self {
        Self::default()
    }

    pub fn sheet_index(&self, sheet_index: usize) -> Option<Arc<SearchSheetIndex>> {
        self.index.fresh_sheet_index(sheet_index)
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
