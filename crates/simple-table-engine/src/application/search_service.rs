use std::sync::Arc;

use crate::application::search_ports::SearchQueryPort;
use crate::domain::{SearchOutcome, SearchScope};
use crate::error::AppError;

#[derive(Clone)]
pub struct SearchService {
    query: Arc<dyn SearchQueryPort>,
}

impl SearchService {
    pub(crate) fn from_port(query: Arc<dyn SearchQueryPort>) -> Self {
        Self { query }
    }

    pub fn search(
        &self,
        document_id: u64,
        base_revision: u64,
        query: &str,
        scope: SearchScope,
        current_sheet_index: Option<usize>,
    ) -> Result<SearchOutcome, AppError> {
        self.query.search(
            document_id,
            base_revision,
            query,
            scope,
            current_sheet_index,
        )
    }

    #[cfg(test)]
    pub(crate) fn is_isolated_from(&self, other: &Self) -> bool {
        !Arc::ptr_eq(&self.query, &other.query)
    }
}
