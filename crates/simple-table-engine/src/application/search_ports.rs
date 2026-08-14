#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use crate::domain::SearchCellText;
use crate::domain::{
    SearchDocumentSnapshot, SearchIndexWork, SearchOutcome, SearchScanCursor, SearchScope,
    SearchTextChunk,
};
use crate::error::AppError;

pub(crate) trait SearchDocumentSourcePort: Send + Sync {
    fn document_snapshot(
        &self,
        document_id: u64,
        expected_revision: Option<u64>,
    ) -> Result<Option<SearchDocumentSnapshot>, AppError>;

    #[cfg(not(target_arch = "wasm32"))]
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

pub(crate) trait SearchQueryPort: Send + Sync {
    fn search(
        &self,
        document_id: u64,
        base_revision: u64,
        query: &str,
        scope: SearchScope,
        current_sheet_index: Option<usize>,
    ) -> Result<SearchOutcome, AppError>;
}

pub(crate) trait SearchIndexMaintenancePort: Send + Sync {
    fn rebuild_all_sheets_index(&self, document_id: u64);

    fn schedule_work(&self, document_id: u64, source_revision: u64, work: SearchIndexWork);

    fn cancel_document_jobs(&self, document_id: u64);
}

#[cfg(test)]
pub(crate) struct NoopSearchIndexMaintenancePort;

#[cfg(test)]
impl SearchIndexMaintenancePort for NoopSearchIndexMaintenancePort {
    fn rebuild_all_sheets_index(&self, _document_id: u64) {}

    fn schedule_work(&self, _document_id: u64, _source_revision: u64, _work: SearchIndexWork) {}

    fn cancel_document_jobs(&self, _document_id: u64) {}
}
