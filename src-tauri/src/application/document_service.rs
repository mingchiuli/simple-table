use std::sync::Arc;

use crate::application::document_open_service::{self, DocumentOpenService};
use crate::application::document_query_service;
use crate::application::mutation_replay::{self, MutationReplayCoordinator};
use crate::application::prepared_document_repository::PreparedDocumentRepository;
use crate::application::search_service::SearchService;
use crate::error::AppError;
use crate::state::state::ActiveDocumentRepository;
use crate::types::OpenDocumentResponse;

#[derive(Clone)]
pub struct DocumentLifecycleService {
    documents: ActiveDocumentRepository,
    prepared_documents: PreparedDocumentRepository,
    mutation_replays: Arc<MutationReplayCoordinator>,
    search: SearchService,
    open_documents: DocumentOpenService,
}

impl DocumentLifecycleService {
    pub(crate) fn new(
        documents: ActiveDocumentRepository,
        prepared_documents: PreparedDocumentRepository,
        mutation_replays: Arc<MutationReplayCoordinator>,
        search: SearchService,
        open_documents: DocumentOpenService,
    ) -> Self {
        Self {
            documents,
            prepared_documents,
            mutation_replays,
            search,
            open_documents,
        }
    }

    fn documents(&self) -> &ActiveDocumentRepository {
        &self.documents
    }

    fn prepared_documents(&self) -> &PreparedDocumentRepository {
        &self.prepared_documents
    }

    fn mutation_replays(&self) -> &Arc<MutationReplayCoordinator> {
        &self.mutation_replays
    }

    fn search(&self) -> &SearchService {
        &self.search
    }

    fn open_documents(&self) -> &DocumentOpenService {
        &self.open_documents
    }
}

/// Commits a prepared document and retires every service resource owned by the
/// previous document before its state is released.
pub fn commit_prepared_document(
    service: &DocumentLifecycleService,
    token: &str,
    expected_document_id: Option<u64>,
    expected_revision: Option<u64>,
) -> Result<OpenDocumentResponse, AppError> {
    let checkout = service.prepared_documents().checkout(token)?;
    let replacement = service
        .documents()
        .begin_replacement(expected_document_id, expected_revision)?;
    document_open_service::adopt_source_path_if_transient(
        service.open_documents(),
        checkout.document().source_path.as_deref(),
        &checkout.document().editor_state.file_data().file_name,
    )?;
    let (prepared, _prepared_commit) = checkout.commit();
    let replacement = replacement.finish(prepared.editor_state)?;
    let document_id = replacement.document_id;
    let previous_document = replacement.previous_document;
    let active_handle = replacement.active_handle;

    let response = {
        let editor_state = active_handle.read()?;
        document_query_service::finalize_open_document_response(
            document_query_service::open_document_response_snapshot(&editor_state),
        )
    };

    if let Some(previous_document_id) = previous_document
        .as_ref()
        .map(|handle| handle.document_id())
        && previous_document_id != document_id
    {
        retire_document_runtime(service, previous_document_id);
    }
    drop(previous_document);
    service
        .search()
        .rebuild_all_sheets_index(service.documents(), document_id);
    Ok(response)
}

pub fn close_current_document(
    service: &DocumentLifecycleService,
    document_id: u64,
) -> Result<(), AppError> {
    let closed_document = service.documents().close(document_id)?;
    if let Some(document_id) = closed_document.as_ref().map(|handle| handle.document_id()) {
        retire_document_runtime(service, document_id);
    }
    drop(closed_document);
    Ok(())
}

fn retire_document_runtime(service: &DocumentLifecycleService, document_id: u64) {
    service.search().cancel_document_jobs(document_id);
    mutation_replay::retire_document(service.mutation_replays(), document_id);
}
