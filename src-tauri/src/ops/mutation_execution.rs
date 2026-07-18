use std::ops::Deref;

use crate::state::search_scheduler::SearchIndexWork;
use crate::types::EditorMutationResponse;

#[derive(Debug)]
pub struct MutationExecution {
    pub response: EditorMutationResponse,
    pub search_index_work: SearchIndexWork,
}

impl MutationExecution {
    pub fn new(response: EditorMutationResponse, search_index_work: SearchIndexWork) -> Self {
        Self {
            response,
            search_index_work,
        }
    }
}

impl Deref for MutationExecution {
    type Target = EditorMutationResponse;

    fn deref(&self) -> &Self::Target {
        &self.response
    }
}
