use std::sync::Arc;

use crate::adapters::search_query_engine::{SearchRequest, execute_search};
use crate::application::search_ports::{SearchDocumentSourcePort, SearchQueryPort};
use crate::domain::{SearchOutcome, SearchScope};
use crate::error::AppError;

#[derive(Clone)]
pub struct SearchQueryAdapter {
    source: Arc<dyn SearchDocumentSourcePort>,
}

impl SearchQueryAdapter {
    pub(crate) fn new(source: Arc<dyn SearchDocumentSourcePort>) -> Self {
        Self { source }
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
        execute_search(
            self.source.as_ref(),
            SearchRequest::new(
                document_id,
                base_revision,
                query,
                scope,
                current_sheet_index,
            ),
            |_| None,
            || Ok(()),
            |_| {},
        )
    }
}
