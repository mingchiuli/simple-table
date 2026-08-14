use std::path::Path;
use std::sync::Arc;

use crate::adapters::document_codec_adapter::DocumentCodecAdapter;
use crate::adapters::document_work_budget_adapter::DocumentWorkBudgetAdapter;
use crate::adapters::search_document_source_adapter::RepositorySearchDocumentSource;
use crate::adapters::search_index_adapter::SearchIndexMaintenanceAdapter;
#[cfg(not(target_arch = "wasm32"))]
use crate::adapters::search_index_runtime::SearchIndexRuntime;
use crate::adapters::search_query_adapter::SearchQueryAdapter;
use crate::application::document_file_workflow::DocumentFileWorkflowService;
use crate::application::document_open_service::DocumentOpenService;
use crate::application::document_query_service::DocumentQueryService;
use crate::application::document_save_service::DocumentSaveService;
use crate::application::document_service::DocumentLifecycleService;
use crate::application::editor_command_service::EditorCommandService;
use crate::application::file_operation_replay::FileOperationReplayCoordinator;
use crate::application::image_service::ImageService;
use crate::application::mutation_replay::MutationReplayCoordinator;
use crate::application::prepared_document_repository::PreparedDocumentRepository;
use crate::application::prepared_source_port::{
    NoopPreparedSourceAdoption, PreparedSourceAdoption, PreparedSourceAdoptionPort,
};
use crate::application::search_service::SearchService;
use crate::error::AppError;
use crate::state::ActiveDocumentRepository;

#[derive(Default)]
struct CorePreparedSourceAdapter;

impl PreparedSourceAdoptionPort for CorePreparedSourceAdapter {
    fn begin_adoption(
        &self,
        _source_path: Option<&Path>,
        _file_name: &str,
    ) -> Result<Box<dyn PreparedSourceAdoption>, AppError> {
        Ok(Box::new(NoopPreparedSourceAdoption))
    }
}

#[derive(Clone)]
pub(crate) struct ApplicationRuntime {
    document_queries: DocumentQueryService,
    document_lifecycle: DocumentLifecycleService,
    document_saves: DocumentSaveService,
    editor_commands: EditorCommandService,
    search_queries: SearchService,
    document_files: DocumentFileWorkflowService,
    images: ImageService,
}

impl Default for ApplicationRuntime {
    fn default() -> Self {
        let documents = ActiveDocumentRepository::default();
        let prepared_documents = PreparedDocumentRepository::default();
        let file_operations = FileOperationReplayCoordinator::default();
        let mutation_replays = Arc::new(MutationReplayCoordinator::default());
        let search_source = Arc::new(RepositorySearchDocumentSource::new(documents.clone()));

        #[cfg(not(target_arch = "wasm32"))]
        let (search_query_adapter, search_indexes) = {
            let search_runtime = SearchIndexRuntime::new(search_source);
            (
                Arc::new(SearchQueryAdapter::new(Arc::clone(&search_runtime))),
                Arc::new(SearchIndexMaintenanceAdapter::new(search_runtime)),
            )
        };
        #[cfg(target_arch = "wasm32")]
        let (search_query_adapter, search_indexes) = (
            Arc::new(SearchQueryAdapter::new(search_source)),
            Arc::new(SearchIndexMaintenanceAdapter::new()),
        );

        let search_queries = SearchService::from_port(search_query_adapter);
        let codec = Arc::new(DocumentCodecAdapter);
        let work_budget = Arc::new(DocumentWorkBudgetAdapter::default());
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
        let document_files = DocumentFileWorkflowService::new(document_opens);

        Self {
            document_queries,
            document_lifecycle: DocumentLifecycleService::new(
                documents.clone(),
                prepared_documents,
                Arc::clone(&mutation_replays),
                search_indexes.clone(),
                Arc::new(CorePreparedSourceAdapter),
                file_operations,
            ),
            document_saves,
            editor_commands: EditorCommandService::new(documents, mutation_replays, search_indexes),
            search_queries,
            document_files,
            images: ImageService::default(),
        }
    }
}

impl ApplicationRuntime {
    pub(crate) fn document_queries(&self) -> &DocumentQueryService {
        &self.document_queries
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

    pub(crate) fn search_queries(&self) -> &SearchService {
        &self.search_queries
    }

    pub(crate) fn document_files(&self) -> &DocumentFileWorkflowService {
        &self.document_files
    }

    pub(crate) fn images(&self) -> &ImageService {
        &self.images
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
                .document_files()
                .is_isolated_from(second.document_files())
        );
        assert!(
            first
                .document_saves()
                .is_isolated_from(second.document_saves())
        );
    }
}
