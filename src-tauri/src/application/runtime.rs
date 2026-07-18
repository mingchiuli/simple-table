use std::sync::Arc;

use crate::application::mutation_replay::MutationReplayCoordinator;
use crate::application::prepared_document_repository::PreparedDocumentRepository;
#[cfg(desktop)]
use crate::io::platform::desktop::DesktopFileRuntime;
#[cfg(any(target_os = "android", target_os = "ios", test))]
use crate::io::platform::mobile::MobileFileRuntime;
use crate::io::save_work::SaveWorkCoordinator;
use crate::recent::store::RecentStore;
use crate::state::search_service::SearchService;
use crate::state::state::ActiveDocumentRepository;

#[derive(Clone)]
pub struct ApplicationRuntime {
    documents: ActiveDocumentRepository,
    prepared_documents: PreparedDocumentRepository,
    mutation_replays: Arc<MutationReplayCoordinator>,
    search: SearchService,
    save_work: SaveWorkCoordinator,
    recent_files: RecentStore,
    #[cfg(desktop)]
    desktop_files: DesktopFileRuntime,
    #[cfg(any(target_os = "android", target_os = "ios", test))]
    mobile_files: MobileFileRuntime,
}

impl Default for ApplicationRuntime {
    fn default() -> Self {
        Self {
            documents: ActiveDocumentRepository::default(),
            prepared_documents: PreparedDocumentRepository::default(),
            mutation_replays: Arc::new(MutationReplayCoordinator::default()),
            search: SearchService::new(),
            save_work: SaveWorkCoordinator::default(),
            recent_files: RecentStore::default(),
            #[cfg(desktop)]
            desktop_files: DesktopFileRuntime::default(),
            #[cfg(any(target_os = "android", target_os = "ios", test))]
            mobile_files: MobileFileRuntime::default(),
        }
    }
}

impl ApplicationRuntime {
    pub(crate) fn documents(&self) -> &ActiveDocumentRepository {
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

    pub(crate) fn save_work(&self) -> &SaveWorkCoordinator {
        &self.save_work
    }

    pub(crate) fn recent_files(&self) -> &RecentStore {
        &self.recent_files
    }

    #[cfg(desktop)]
    pub(crate) fn desktop_files(&self) -> &DesktopFileRuntime {
        &self.desktop_files
    }

    #[cfg(any(target_os = "android", target_os = "ios", test))]
    pub(crate) fn mobile_files(&self) -> &MobileFileRuntime {
        &self.mobile_files
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_runtimes_do_not_share_business_state() {
        let first = ApplicationRuntime::default();
        let second = ApplicationRuntime::default();

        assert!(!first.documents().is_same_instance(second.documents()));
        assert!(!Arc::ptr_eq(
            first.mutation_replays(),
            second.mutation_replays()
        ));
        assert!(
            !first
                .prepared_documents()
                .is_same_instance(second.prepared_documents())
        );
        assert!(!first.save_work().is_same_instance(second.save_work()));
        assert!(!first.recent_files().is_same_instance(second.recent_files()));
        #[cfg(desktop)]
        assert!(
            !first
                .desktop_files()
                .is_same_instance(second.desktop_files())
        );
        #[cfg(any(target_os = "android", target_os = "ios", test))]
        assert!(first.mobile_files().is_isolated_from(second.mobile_files()));
    }
}
