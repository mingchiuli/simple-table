use std::sync::Arc;

use crate::adapters::search_index_runtime::SearchIndexRuntime;
use crate::application::search_ports::SearchIndexMaintenancePort;
use crate::domain::SearchIndexWork;

#[derive(Clone)]
pub struct SearchIndexMaintenanceAdapter {
    runtime: Arc<SearchIndexRuntime>,
}

impl SearchIndexMaintenanceAdapter {
    pub(crate) fn new(runtime: Arc<SearchIndexRuntime>) -> Self {
        Self { runtime }
    }
}

impl SearchIndexMaintenancePort for SearchIndexMaintenanceAdapter {
    fn rebuild_all_sheets_index(&self, document_id: u64) {
        self.runtime.rebuild_all_sheets_index(document_id);
    }

    fn schedule_work(&self, document_id: u64, source_revision: u64, work: SearchIndexWork) {
        self.runtime
            .schedule_work(document_id, source_revision, work);
    }

    fn cancel_document_jobs(&self, document_id: u64) {
        self.runtime.cancel_document_jobs(document_id);
    }
}
