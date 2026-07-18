use std::sync::Arc;

use crate::application::search_ports::SearchIndexPort;
use crate::domain::SearchIndexWork;
use crate::error::AppError;
use crate::types::{SearchResponse, SearchScope};

#[derive(Clone)]
pub struct SearchService {
    indexes: Arc<dyn SearchIndexPort>,
}

impl SearchService {
    pub(crate) fn from_port(indexes: Arc<dyn SearchIndexPort>) -> Self {
        Self { indexes }
    }

    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::from_port(Arc::new(NoopSearchIndexPort))
    }

    pub fn search(
        &self,
        document_id: u64,
        base_revision: u64,
        query: &str,
        scope: SearchScope,
        current_sheet_index: Option<usize>,
    ) -> Result<SearchResponse, AppError> {
        self.indexes.search(
            document_id,
            base_revision,
            query,
            scope,
            current_sheet_index,
        )
    }

    pub fn rebuild_all_sheets_index(&self, document_id: u64) {
        self.indexes.rebuild_all_sheets_index(document_id);
    }

    pub fn schedule_work(&self, document_id: u64, source_revision: u64, work: SearchIndexWork) {
        self.indexes
            .schedule_work(document_id, source_revision, work);
    }

    pub fn cancel_document_jobs(&self, document_id: u64) {
        self.indexes.cancel_document_jobs(document_id);
    }

    #[cfg(test)]
    pub(crate) fn is_isolated_from(&self, other: &Self) -> bool {
        !Arc::ptr_eq(&self.indexes, &other.indexes)
    }
}

#[cfg(test)]
struct NoopSearchIndexPort;

#[cfg(test)]
impl SearchIndexPort for NoopSearchIndexPort {
    fn search(
        &self,
        _document_id: u64,
        _base_revision: u64,
        _query: &str,
        _scope: SearchScope,
        _current_sheet_index: Option<usize>,
    ) -> Result<SearchResponse, AppError> {
        Ok(SearchResponse::default())
    }

    fn rebuild_all_sheets_index(&self, _document_id: u64) {}

    fn schedule_work(&self, _document_id: u64, _source_revision: u64, _work: SearchIndexWork) {}

    fn cancel_document_jobs(&self, _document_id: u64) {}
}
