use crate::application::search_ports::SearchIndexMaintenancePort;
use crate::domain::SearchIndexWork;

#[derive(Default)]
pub struct SearchIndexMaintenanceAdapter;

impl SearchIndexMaintenanceAdapter {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl SearchIndexMaintenancePort for SearchIndexMaintenanceAdapter {
    fn rebuild_all_sheets_index(&self, _document_id: u64) {}

    fn schedule_work(&self, _document_id: u64, _source_revision: u64, _work: SearchIndexWork) {}

    fn cancel_document_jobs(&self, _document_id: u64) {}
}
