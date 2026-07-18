use std::sync::Arc;

use crate::adapters::document_codec_adapter::DocumentCodecAdapter;
use crate::adapters::document_file_adapter::DocumentFileAdapter;
use crate::adapters::document_work_budget_adapter::DocumentWorkBudgetAdapter;
use crate::adapters::recent_file_adapter::RecentFileAdapter;
use crate::adapters::search_document_source_adapter::RepositorySearchDocumentSource;
use crate::adapters::search_index_adapter::SearchIndexAdapter;
use crate::application::document_open_service::DocumentOpenService;
use crate::application::document_query_service::DocumentQueryService;
use crate::application::document_save_service::DocumentSaveService;
use crate::application::document_service::DocumentLifecycleService;
use crate::application::editor_command_service::EditorCommandService;
use crate::application::mutation_replay::MutationReplayCoordinator;
use crate::application::prepared_document_repository::PreparedDocumentRepository;
use crate::application::search_service::SearchService;
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
    document_files: DocumentFileAdapter,
    recent_files: RecentFileAdapter,
}

impl Default for ApplicationRuntime {
    fn default() -> Self {
        let documents = ActiveDocumentRepository::default();
        let prepared_documents = PreparedDocumentRepository::default();
        let mutation_replays = Arc::new(MutationReplayCoordinator::default());
        let search_source = Arc::new(RepositorySearchDocumentSource::new(documents.clone()));
        let search_indexes = Arc::new(SearchIndexAdapter::new(search_source));
        let search_queries = SearchService::from_port(search_indexes.clone());
        let codec = Arc::new(DocumentCodecAdapter);
        let work_budget = Arc::new(DocumentWorkBudgetAdapter::default());
        let recent_files = RecentStore::default();
        #[cfg(desktop)]
        let desktop_files = DesktopFileRuntime::default();
        #[cfg(any(target_os = "android", target_os = "ios", test))]
        let mobile_files = MobileFileRuntime::default();
        let document_opens =
            DocumentOpenService::new(documents.clone(), prepared_documents.clone(), codec.clone());
        let document_queries = DocumentQueryService::new(documents.clone());
        let document_saves = DocumentSaveService::new(
            documents.clone(),
            search_indexes.clone(),
            codec.clone(),
            codec,
            work_budget,
        );
        #[cfg(any(target_os = "android", target_os = "ios", test))]
        let prepared_source_adopter = {
            let mobile_files = mobile_files.clone();
            Arc::new(
                move |source_path: Option<&std::path::Path>, file_name: &str| {
                    if let Some(source_path) = source_path {
                        crate::io::managed_documents::adopt_transient_document(
                            mobile_files.managed_documents(),
                            mobile_files.transient_files(),
                            source_path,
                            file_name,
                        )?;
                    }
                    Ok(())
                },
            ) as crate::application::document_service::PreparedSourceAdopter
        };
        #[cfg(not(any(target_os = "android", target_os = "ios", test)))]
        let prepared_source_adopter =
            Arc::new(|_source_path: Option<&std::path::Path>, _file_name: &str| Ok(()))
                as crate::application::document_service::PreparedSourceAdopter;
        let document_files = DocumentFileAdapter::new(
            document_opens.clone(),
            document_saves.clone(),
            #[cfg(desktop)]
            recent_files.clone(),
            #[cfg(desktop)]
            desktop_files,
            #[cfg(any(target_os = "android", target_os = "ios", test))]
            mobile_files.clone(),
        );
        Self {
            document_queries: document_queries.clone(),
            document_opens: document_opens.clone(),
            document_lifecycle: DocumentLifecycleService::new(
                documents.clone(),
                prepared_documents,
                Arc::clone(&mutation_replays),
                search_indexes.clone(),
                prepared_source_adopter,
            ),
            editor_commands: EditorCommandService::new(documents, mutation_replays, search_indexes),
            search_queries,
            document_files,
            recent_files: RecentFileAdapter::new(
                document_queries,
                recent_files.clone(),
                #[cfg(any(target_os = "android", target_os = "ios"))]
                mobile_files.clone(),
            ),
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

    pub(crate) fn document_files(&self) -> &DocumentFileAdapter {
        &self.document_files
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
                .document_files()
                .is_isolated_from(second.document_files())
        );
    }
}
