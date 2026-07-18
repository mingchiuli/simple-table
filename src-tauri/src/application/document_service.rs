use crate::application::runtime::ApplicationRuntime;
use crate::application::{document_open_service, document_query_service, mutation_replay};
use crate::error::AppError;
use crate::types::OpenDocumentResponse;

/// Commits a prepared document and retires every runtime resource owned by the
/// previous document before its state is released.
pub fn commit_prepared_document(
    runtime: &ApplicationRuntime,
    token: &str,
    expected_document_id: Option<u64>,
    expected_revision: Option<u64>,
) -> Result<OpenDocumentResponse, AppError> {
    let checkout = runtime.prepared_documents().checkout(token)?;
    let replacement = runtime
        .documents()
        .begin_replacement(expected_document_id, expected_revision)?;
    document_open_service::adopt_source_path_if_transient(
        runtime,
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
        retire_document_runtime(runtime, previous_document_id);
    }
    drop(previous_document);
    runtime
        .search()
        .rebuild_all_sheets_index(runtime.documents(), document_id);
    Ok(response)
}

pub fn close_current_document(
    runtime: &ApplicationRuntime,
    document_id: u64,
) -> Result<(), AppError> {
    let closed_document = runtime.documents().close(document_id)?;
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
