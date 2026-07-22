use std::sync::Arc;

use crate::adapters::document_codec_adapter::DocumentCodecAdapter;
use crate::adapters::document_file_adapter::PlatformFileAdapter;
use crate::adapters::document_work_budget_adapter::DocumentWorkBudgetAdapter;
use crate::adapters::recent_file_adapter::RecentFileAdapter;
use crate::adapters::search_document_source_adapter::RepositorySearchDocumentSource;
use crate::adapters::search_index_adapter::SearchIndexMaintenanceAdapter;
use crate::adapters::search_index_runtime::SearchIndexRuntime;
use crate::adapters::search_query_adapter::SearchQueryAdapter;
#[cfg(any(target_os = "android", target_os = "ios", test))]
use crate::adapters::update_adapter::UpdateReleaseAdapter;
use crate::application::document_file_workflow::DocumentFileWorkflowService;
use crate::application::document_open_service::DocumentOpenService;
use crate::application::document_query_service::DocumentQueryService;
use crate::application::document_save_service::DocumentSaveService;
use crate::application::document_service::DocumentLifecycleService;
use crate::application::editor_command_service::EditorCommandService;
use crate::application::mutation_replay::MutationReplayCoordinator;
use crate::application::prepared_document_repository::PreparedDocumentRepository;
use crate::application::search_service::SearchService;
#[cfg(any(target_os = "android", target_os = "ios", test))]
use crate::application::update_service::UpdateService;
#[cfg(desktop)]
use crate::io::platform::desktop::DesktopFileRuntime;
#[cfg(any(target_os = "android", target_os = "ios", test))]
use crate::io::platform::mobile::MobileFileRuntime;
use crate::recent::store::RecentStore;
use crate::state::state::ActiveDocumentRepository;

#[derive(Clone)]
pub struct ApplicationRuntime {
    document_queries: DocumentQueryService,
    document_opens: DocumentOpenService,
    document_lifecycle: DocumentLifecycleService,
    editor_commands: EditorCommandService,
    search_queries: SearchService,
    document_files: DocumentFileWorkflowService,
    platform_files: Arc<PlatformFileAdapter>,
    recent_files: RecentFileAdapter,
    #[cfg(any(target_os = "android", target_os = "ios", test))]
    update_queries: UpdateService,
}

impl Default for ApplicationRuntime {
    fn default() -> Self {
        let documents = ActiveDocumentRepository::default();
        let prepared_documents = PreparedDocumentRepository::default();
        let mutation_replays = Arc::new(MutationReplayCoordinator::default());
        let search_source = Arc::new(RepositorySearchDocumentSource::new(documents.clone()));
        let search_runtime = SearchIndexRuntime::new(search_source);
        let search_query_adapter = Arc::new(SearchQueryAdapter::new(Arc::clone(&search_runtime)));
        let search_indexes = Arc::new(SearchIndexMaintenanceAdapter::new(search_runtime));
        let search_queries = SearchService::from_port(search_query_adapter);
        let codec = Arc::new(DocumentCodecAdapter);
        let work_budget = Arc::new(DocumentWorkBudgetAdapter::default());
        let recent_files = RecentStore::default();
        #[cfg(any(target_os = "android", target_os = "ios", test))]
        let update_queries = UpdateService::new(Arc::new(UpdateReleaseAdapter::default()));
        #[cfg(desktop)]
        let desktop_files = DesktopFileRuntime::default();
        #[cfg(any(target_os = "android", target_os = "ios", test))]
        let mobile_files = MobileFileRuntime::default();
        let document_opens = DocumentOpenService::new(
            documents.clone(),
            prepared_documents.clone(),
            codec.clone(),
            work_budget.clone(),
        );
        let document_queries = DocumentQueryService::new(documents.clone());
        let document_saves = DocumentSaveService::new(
            documents.clone(),
            search_indexes.clone(),
            codec.clone(),
            codec,
            work_budget,
        );
        let platform_files = Arc::new(PlatformFileAdapter::new(
            #[cfg(desktop)]
            recent_files.clone(),
            #[cfg(desktop)]
            desktop_files,
            #[cfg(any(target_os = "android", target_os = "ios", test))]
            mobile_files.clone(),
        ));
        let document_files =
            DocumentFileWorkflowService::new(document_opens.clone(), document_saves);
        Self {
            document_queries: document_queries.clone(),
            document_opens: document_opens.clone(),
            document_lifecycle: DocumentLifecycleService::new(
                documents.clone(),
                prepared_documents,
                Arc::clone(&mutation_replays),
                search_indexes.clone(),
                platform_files.clone(),
            ),
            editor_commands: EditorCommandService::new(documents, mutation_replays, search_indexes),
            search_queries,
            document_files,
            platform_files,
            recent_files: RecentFileAdapter::new(
                document_queries,
                recent_files.clone(),
                #[cfg(any(target_os = "android", target_os = "ios"))]
                mobile_files.clone(),
            ),
            #[cfg(any(target_os = "android", target_os = "ios", test))]
            update_queries,
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

    pub(crate) fn editor_commands(&self) -> &EditorCommandService {
        &self.editor_commands
    }

    pub(crate) fn search_queries(&self) -> &SearchService {
        &self.search_queries
    }

    pub(crate) fn recent_files(&self) -> &RecentFileAdapter {
        &self.recent_files
    }

    pub(crate) fn document_files(&self) -> &DocumentFileWorkflowService {
        &self.document_files
    }

    pub(crate) fn platform_files(&self) -> &PlatformFileAdapter {
        &self.platform_files
    }

    #[cfg(any(target_os = "android", target_os = "ios", test))]
    pub(crate) fn update_queries(&self) -> &UpdateService {
        &self.update_queries
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
                .search_queries()
                .is_isolated_from(second.search_queries())
        );
        assert!(
            first
                .document_opens()
                .is_isolated_from(second.document_opens())
        );
        assert!(first.recent_files().is_isolated_from(second.recent_files()));
        assert!(
            first
                .update_queries()
                .is_isolated_from(second.update_queries())
        );
        assert!(
            first
                .document_files()
                .is_isolated_from(second.document_files())
        );
        assert!(
            first
                .platform_files()
                .is_isolated_from(second.platform_files())
        );
    }
}
