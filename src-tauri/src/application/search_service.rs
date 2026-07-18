use std::sync::Arc;

use crate::domain::SearchIndexWork;
use crate::error::AppError;
use crate::state::state::ActiveDocumentRepository;
use crate::types::{SearchResponse, SearchScope};

pub(crate) trait SearchScanLease: Send {}

pub(crate) trait SearchIndexPort: Send + Sync {
    fn reserve_scan_work(&self) -> Result<Box<dyn SearchScanLease>, AppError>;
    fn rebuild_all_sheets_index(&self, registry: &ActiveDocumentRepository, document_id: u64);
    fn rebuild_sheet_index(
        &self,
        registry: &ActiveDocumentRepository,
        document_id: u64,
        sheet_index: usize,
    );
    fn schedule_work(
        &self,
        document_id: u64,
        work: SearchIndexWork,
        registry: &ActiveDocumentRepository,
    );
    fn cancel_document_jobs(&self, document_id: u64);
}

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
        registry: &ActiveDocumentRepository,
        document_id: u64,
        base_revision: u64,
        query: &str,
        scope: SearchScope,
        current_sheet_index: Option<usize>,
    ) -> Result<SearchResponse, AppError> {
        crate::ops::search_ops::do_search(
            registry,
            document_id,
            base_revision,
            query,
            scope,
            current_sheet_index,
            || self.indexes.reserve_scan_work(),
            |sheet_index| {
                self.indexes
                    .rebuild_sheet_index(registry, document_id, sheet_index);
            },
        )
    }

    pub fn rebuild_all_sheets_index(&self, registry: &ActiveDocumentRepository, document_id: u64) {
        self.indexes.rebuild_all_sheets_index(registry, document_id);
    }

    pub fn schedule_work(
        &self,
        document_id: u64,
        work: SearchIndexWork,
        registry: &ActiveDocumentRepository,
    ) {
        self.indexes.schedule_work(document_id, work, registry);
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
struct NoopSearchScanLease;

#[cfg(test)]
impl SearchScanLease for NoopSearchScanLease {}

#[cfg(test)]
impl SearchIndexPort for NoopSearchIndexPort {
    fn reserve_scan_work(&self) -> Result<Box<dyn SearchScanLease>, AppError> {
        Ok(Box::new(NoopSearchScanLease))
    }

    fn rebuild_all_sheets_index(&self, _registry: &ActiveDocumentRepository, _document_id: u64) {}

    fn rebuild_sheet_index(
        &self,
        _registry: &ActiveDocumentRepository,
        _document_id: u64,
        _sheet_index: usize,
    ) {
    }

    fn schedule_work(
        &self,
        _document_id: u64,
        _work: SearchIndexWork,
        _registry: &ActiveDocumentRepository,
    ) {
    }

    fn cancel_document_jobs(&self, _document_id: u64) {}
}
