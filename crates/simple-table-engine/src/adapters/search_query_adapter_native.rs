use std::sync::Arc;

use crate::adapters::search_index_runtime::SearchIndexRuntime;
use crate::application::search_ports::SearchQueryPort;
use crate::domain::{SearchOutcome, SearchScope};
use crate::error::AppError;

#[derive(Clone)]
pub struct SearchQueryAdapter {
    runtime: Arc<SearchIndexRuntime>,
}

impl SearchQueryAdapter {
    pub(crate) fn new(runtime: Arc<SearchIndexRuntime>) -> Self {
        Self { runtime }
    }
}

impl SearchQueryPort for SearchQueryAdapter {
    fn search(
        &self,
        document_id: u64,
        base_revision: u64,
        query: &str,
        scope: SearchScope,
        current_sheet_index: Option<usize>,
    ) -> Result<SearchOutcome, AppError> {
        self.runtime.search(
            document_id,
            base_revision,
            query,
            scope,
            current_sheet_index,
        )
    }
}
