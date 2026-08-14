use crate::domain::SearchIndexWork;
use crate::projection_model::MutationOutcome;

#[derive(Debug)]
pub struct MutationExecution {
    pub outcome: MutationOutcome,
    pub search_index_work: SearchIndexWork,
}

impl MutationExecution {
    pub fn new(outcome: MutationOutcome, search_index_work: SearchIndexWork) -> Self {
        Self {
            outcome,
            search_index_work,
        }
    }
}
