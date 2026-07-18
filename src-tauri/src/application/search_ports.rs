use std::sync::Arc;

use crate::domain::{
    SearchCellText, SearchDocumentSnapshot, SearchIndexWork, SearchScanCursor, SearchTextChunk,
};
use crate::error::AppError;
use crate::types::{SearchResponse, SearchScope};

pub(crate) trait SearchDocumentSourcePort: Send + Sync {
    fn document_snapshot(
        &self,
        document_id: u64,
        expected_revision: Option<u64>,
    ) -> Result<Option<SearchDocumentSnapshot>, AppError>;

    fn sheet_text_snapshot(
        &self,
        document_id: u64,
        expected_revision: u64,
        sheet_index: usize,
    ) -> Result<Option<Arc<[SearchCellText]>>, AppError>;

    fn sheet_text_chunk(
        &self,
        document_id: u64,
        expected_revision: u64,
        sheet_index: usize,
        cursor: SearchScanCursor,
        maximum_text_bytes: usize,
        maximum_cells: usize,
    ) -> Result<Option<SearchTextChunk>, AppError>;
}

pub(crate) trait SearchIndexPort: Send + Sync {
    fn search(
        &self,
        document_id: u64,
        base_revision: u64,
        query: &str,
        scope: SearchScope,
        current_sheet_index: Option<usize>,
    ) -> Result<SearchResponse, AppError>;

    fn rebuild_all_sheets_index(&self, document_id: u64);

    fn schedule_work(&self, document_id: u64, source_revision: u64, work: SearchIndexWork);

    fn cancel_document_jobs(&self, document_id: u64);
}
