use std::sync::{Arc, RwLock};

use crate::application::runtime::ApplicationRuntime;
use crate::application::{document_open_service, document_query_service, mutation_replay};
use crate::error::AppError;
use crate::state::state::{ActiveDocumentStore, DocumentReplacementLease};
use crate::types::OpenDocumentResponse;

/// Commits a prepared document and retires every runtime resource owned by the
/// previous document before its state is released.
pub fn commit_prepared_document(
    runtime: &ApplicationRuntime,
    token: &str,
    expected_document_id: Option<u64>,
    expected_revision: Option<u64>,
) -> Result<OpenDocumentResponse, AppError> {
    let registry = runtime.documents();
    let checkout = runtime.prepared_documents().checkout(token)?;
    let replacement_lease = {
        let mut registry_guard = registry
            .write()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        registry_guard.begin_document_replacement(expected_document_id, expected_revision)?
    };
    let mut replacement = ActiveDocumentReplacement::new(&registry, replacement_lease);
    document_open_service::adopt_source_path_if_transient(
        checkout.document().source_path.as_deref(),
        &checkout.document().editor_state.file_data().file_name,
    )?;
    let (prepared, _prepared_commit) = checkout.commit();
    let (document_id, previous_document, active_handle) = {
        let mut registry_guard = registry
            .write()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        let (document_id, previous_document) =
            registry_guard.finish_document_replacement(replacement_lease, prepared.editor_state)?;
        replacement.finished = true;
        (
            document_id,
            previous_document,
            registry_guard
                .active_handle()
                .ok_or(AppError::NoFileLoaded)?,
        )
    };

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
        retire_document_runtime(runtime, previous_document_id);
    }
    drop(previous_document);
    runtime
        .search()
        .rebuild_all_sheets_index(registry, document_id);
    Ok(response)
}

pub fn close_current_document(
    runtime: &ApplicationRuntime,
    document_id: u64,
) -> Result<(), AppError> {
    let registry = runtime.documents();
    let closed_document = {
        let mut registry_guard = registry
            .write()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        registry_guard.close_active_document(document_id)?
    };
    if let Some(document_id) = closed_document.as_ref().map(|handle| handle.document_id()) {
        retire_document_runtime(runtime, document_id);
    }
    drop(closed_document);
    Ok(())
}

fn retire_document_runtime(runtime: &ApplicationRuntime, document_id: u64) {
    runtime.search().cancel_document_jobs(document_id);
    mutation_replay::retire_document(runtime.mutation_replays(), document_id);
}

struct ActiveDocumentReplacement<'a> {
    registry: &'a Arc<RwLock<ActiveDocumentStore>>,
    lease: DocumentReplacementLease,
    finished: bool,
}

impl<'a> ActiveDocumentReplacement<'a> {
    fn new(
        registry: &'a Arc<RwLock<ActiveDocumentStore>>,
        lease: DocumentReplacementLease,
    ) -> Self {
        Self {
            registry,
            lease,
            finished: false,
        }
    }
}

impl Drop for ActiveDocumentReplacement<'_> {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        if let Ok(mut registry) = self.registry.write() {
            registry.abort_document_replacement(self.lease);
        }
    }
}
