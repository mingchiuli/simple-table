use std::sync::{Arc, RwLock};

use crate::application::mutation_replay::MutationReplayCoordinator;
use crate::application::prepared_document_repository::PreparedDocumentRepository;
use crate::state::search_service::SearchService;
use crate::state::state::ActiveDocumentStore;

#[derive(Clone)]
pub struct ApplicationRuntime {
    documents: Arc<RwLock<ActiveDocumentStore>>,
    prepared_documents: PreparedDocumentRepository,
    mutation_replays: Arc<MutationReplayCoordinator>,
    search: SearchService,
}

impl Default for ApplicationRuntime {
    fn default() -> Self {
        Self {
            documents: Arc::new(RwLock::new(ActiveDocumentStore::new())),
            prepared_documents: PreparedDocumentRepository::default(),
            mutation_replays: Arc::new(MutationReplayCoordinator::default()),
            search: SearchService::new(),
        }
    }
}

impl ApplicationRuntime {
    pub(crate) fn documents(&self) -> &Arc<RwLock<ActiveDocumentStore>> {
        &self.documents
    }

    pub(crate) fn prepared_documents(&self) -> &PreparedDocumentRepository {
        &self.prepared_documents
    }

    pub(crate) fn mutation_replays(&self) -> &Arc<MutationReplayCoordinator> {
        &self.mutation_replays
    }

    pub(crate) fn search(&self) -> &SearchService {
        &self.search
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_runtimes_do_not_share_business_state() {
        let first = ApplicationRuntime::default();
        let second = ApplicationRuntime::default();

        assert!(!Arc::ptr_eq(first.documents(), second.documents()));
        assert!(!Arc::ptr_eq(
            first.mutation_replays(),
            second.mutation_replays()
        ));
        assert!(
            !first
                .prepared_documents()
                .is_same_instance(second.prepared_documents())
        );
    }
}
