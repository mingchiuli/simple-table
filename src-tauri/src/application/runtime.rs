use std::sync::Arc;

use crate::application::document_open_service::DocumentOpenService;
use crate::application::document_query_service::DocumentQueryService;
use crate::application::document_save_service::DocumentSaveService;
use crate::application::document_service::DocumentLifecycleService;
use crate::application::editor_command_service::EditorCommandService;
use crate::application::mutation_replay::MutationReplayCoordinator;
use crate::application::prepared_document_repository::PreparedDocumentRepository;
use crate::application::recent_file_service::RecentFileService;
use crate::application::search_service::SearchService;
#[cfg(desktop)]
use crate::io::platform::desktop::DesktopFileRuntime;
#[cfg(any(target_os = "android", target_os = "ios", test))]
use crate::io::platform::mobile::MobileFileRuntime;
use crate::io::save_work::SaveWorkCoordinator;
use crate::recent::store::RecentStore;
use crate::state::state::ActiveDocumentRepository;

#[derive(Clone)]
pub struct ApplicationRuntime {
    document_queries: DocumentQueryService,
    document_opens: DocumentOpenService,
    document_lifecycle: DocumentLifecycleService,
    document_saves: DocumentSaveService,
    editor_commands: EditorCommandService,
    recent_files: RecentFileService,
    #[cfg(desktop)]
    desktop_files: DesktopFileRuntime,
    #[cfg(any(target_os = "android", target_os = "ios", test))]
    mobile_files: MobileFileRuntime,
}

impl Default for ApplicationRuntime {
    fn default() -> Self {
        let documents = ActiveDocumentRepository::default();
        let prepared_documents = PreparedDocumentRepository::default();
        let mutation_replays = Arc::new(MutationReplayCoordinator::default());
        let search = SearchService::new();
        let save_work = SaveWorkCoordinator::default();
        let recent_files = RecentStore::default();
        #[cfg(desktop)]
        let desktop_files = DesktopFileRuntime::default();
        #[cfg(any(target_os = "android", target_os = "ios", test))]
        let mobile_files = MobileFileRuntime::default();
        let document_opens = DocumentOpenService::new(
            documents.clone(),
            prepared_documents.clone(),
            recent_files.clone(),
            #[cfg(desktop)]
            desktop_files.clone(),
            #[cfg(any(target_os = "android", target_os = "ios", test))]
            mobile_files.clone(),
        );
        let document_queries = DocumentQueryService::new(documents.clone());
        Self {
            document_queries: document_queries.clone(),
            document_opens: document_opens.clone(),
            document_lifecycle: DocumentLifecycleService::new(
                documents.clone(),
                prepared_documents,
                Arc::clone(&mutation_replays),
                search.clone(),
                document_opens,
            ),
            document_saves: DocumentSaveService::new(
                documents.clone(),
                search.clone(),
                save_work,
                #[cfg(desktop)]
                desktop_files.clone(),
                #[cfg(any(target_os = "android", target_os = "ios"))]
                mobile_files.clone(),
            ),
            editor_commands: EditorCommandService::new(documents, mutation_replays, search),
            recent_files: RecentFileService::new(
                document_queries,
                recent_files,
                #[cfg(any(target_os = "android", target_os = "ios"))]
                mobile_files.clone(),
            ),
            #[cfg(desktop)]
            desktop_files,
            #[cfg(any(target_os = "android", target_os = "ios", test))]
            mobile_files,
        }
    }
}

impl ApplicationRuntime {
    pub(crate) fn document_queries(&self) -> &DocumentQueryService {
        &self.document_queries
    }

    pub(crate) fn document_opens(&self) -> &DocumentOpenService {
        &self.document_opens
    }

    pub(crate) fn document_lifecycle(&self) -> &DocumentLifecycleService {
        &self.document_lifecycle
    }

    pub(crate) fn document_saves(&self) -> &DocumentSaveService {
        &self.document_saves
    }

    pub(crate) fn editor_commands(&self) -> &EditorCommandService {
        &self.editor_commands
    }

    pub(crate) fn recent_files(&self) -> &RecentFileService {
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

        assert!(
            first
                .editor_commands()
                .is_isolated_from(second.editor_commands())
        );
        assert!(
            first
                .document_opens()
                .is_isolated_from(second.document_opens())
        );
        assert!(
            first
                .document_saves()
                .is_isolated_from(second.document_saves())
        );
        assert!(first.recent_files().is_isolated_from(second.recent_files()));
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
