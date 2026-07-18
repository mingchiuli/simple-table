use std::sync::Arc;

use crate::application::document_query_service;
use crate::application::runtime::ApplicationRuntime;
use crate::error::AppError;
use crate::io::codec::reader::read_file_with_workbook_from_bytes;
use crate::io::file_format::{default_spreadsheet_extension, extension_of, is_xlsx_extension};
use crate::io::save_work::SaveWorkReservation;
use crate::state::{
    editor_state::{EditorState, SaveCommitLease},
    search_service::SearchService,
    state::{ActiveDocumentRepository, DocumentHandle},
};
use crate::types::{SavedDocumentIdentity, SavedDocumentResponse};

pub struct PreparedDocumentExport {
    pub bytes: Vec<u8>,
    _work: Option<SaveWorkReservation>,
}

pub struct PreparedDocumentSave {
    pub document_id: u64,
    pub revision: u64,
    pub output_name: String,
    pub bytes: Vec<u8>,
    pub finish_without_reparse: bool,
    _work: Option<SaveWorkReservation>,
}

pub fn prepare_current_file_export(
    runtime: &ApplicationRuntime,
    document_id: u64,
    base_revision: u64,
    target_path_or_name: &str,
) -> Result<PreparedDocumentExport, AppError> {
    let handle = document_handle_for_read(runtime.documents(), document_id)?;
    let (snapshot, work) = {
        let editor_state = handle.read_for_command(document_id, base_revision)?;
        let work = runtime
            .save_work()
            .reserve(document_id, editor_state.estimated_resource_bytes())?;
        let snapshot = editor_state.save_snapshot_for_target(target_path_or_name)?;
        (snapshot, work)
    };
    let (_, bytes) = snapshot.generate_file_bytes_for_target(target_path_or_name)?;
    Ok(PreparedDocumentExport {
        bytes,
        _work: Some(work),
    })
}

pub fn prepare_current_file_save(
    runtime: &ApplicationRuntime,
    document_id: u64,
    base_revision: u64,
    target_path_or_name: &str,
) -> Result<PreparedDocumentSave, AppError> {
    let (snapshot, work) = {
        let handle = document_handle_for_read(runtime.documents(), document_id)?;
        let editor_state = handle.read_for_command(document_id, base_revision)?;
        document_query_service::ensure_native_save_target_allowed(
            &editor_state,
            target_path_or_name,
        )?;
        if editor_state.has_save_commit_in_progress() {
            return Err(AppError::DocumentStateInvalid(
                "save is already in progress".to_string(),
            ));
        }
        let work = runtime
            .save_work()
            .reserve(document_id, editor_state.estimated_resource_bytes())?;
        let snapshot = editor_state.save_snapshot_for_target(target_path_or_name)?;
        (snapshot, work)
    };

    let (output_name, bytes) = snapshot.generate_file_bytes_for_target(target_path_or_name)?;
    let target_extension = extension_of(&output_name)
        .or_else(|| extension_of(target_path_or_name))
        .unwrap_or_else(default_extension_string);
    Ok(PreparedDocumentSave {
        document_id,
        revision: base_revision,
        output_name,
        bytes,
        finish_without_reparse: is_xlsx_extension(&target_extension) && snapshot.is_excel_backed(),
        _work: Some(work),
    })
}

pub fn abort_prepared_file_save(_prepared: PreparedDocumentSave) {}

pub fn commit_current_file_save<F>(
    runtime: &ApplicationRuntime,
    path: String,
    prepared: PreparedDocumentSave,
    commit_write: F,
) -> Result<SavedDocumentResponse, AppError>
where
    F: FnOnce() -> Result<(), AppError>,
{
    commit_current_file_save_with_registry(
        runtime.search(),
        runtime.documents(),
        path,
        prepared,
        commit_write,
    )
}

fn commit_current_file_save_with_registry<F>(
    search: &SearchService,
    registry: &ActiveDocumentRepository,
    path: String,
    prepared: PreparedDocumentSave,
    commit_write: F,
) -> Result<SavedDocumentResponse, AppError>
where
    F: FnOnce() -> Result<(), AppError>,
{
    let PreparedDocumentSave {
        document_id,
        revision,
        output_name,
        bytes,
        finish_without_reparse,
        _work,
    } = prepared;
    let extension = extension_of(&output_name)
        .or_else(|| extension_of(&path))
        .unwrap_or_else(default_extension_string);
    if finish_without_reparse {
        return commit_current_file_save_without_reparse(
            registry,
            path,
            document_id,
            revision,
            output_name,
            extension,
            commit_write,
        );
    }

    let result = read_file_with_workbook_from_bytes(&extension, bytes, path.clone(), output_name)?;
    let (handle, lease, clear_history) =
        begin_prepared_save_commit(registry, document_id, revision, |editor_state| {
            let current_extension = extension_of(&editor_state.file_data().file_name)
                .or_else(|| extension_of(&editor_state.file_data().path));
            let saved_extension = extension_of(&result.file_data.file_name)
                .or_else(|| extension_of(&result.file_data.path));
            current_extension != saved_extension
        })?;

    if let Err(error) = commit_write() {
        abort_save_commit(&handle, lease);
        return Err(error);
    }

    let (document_id, response, retired) = {
        let mut editor_state = handle.write()?;
        let retired = editor_state.finish_save_commit(
            lease,
            result.file_data,
            result.workbook,
            clear_history,
        )?;
        let response = SavedDocumentResponse {
            document: Some(document_query_service::document_manifest(&editor_state)),
            identity: None,
            editor_session: document_query_service::editor_session_info(&editor_state),
        };
        (editor_state.document_id(), response, retired)
    };
    drop(retired);
    search.rebuild_all_sheets_index(registry, document_id);
    Ok(response)
}

fn begin_prepared_save_commit<F>(
    registry: &ActiveDocumentRepository,
    document_id: u64,
    revision: u64,
    clear_history: F,
) -> Result<(Arc<DocumentHandle>, SaveCommitLease, bool), AppError>
where
    F: FnOnce(&EditorState) -> bool,
{
    let handle = registry.mutation_handle(document_id)?;
    let mut editor_state = handle.write()?;
    ensure_editor_matches_prepared_save(&editor_state, document_id, revision)?;
    let clear_history = clear_history(&editor_state);
    let lease = editor_state.begin_save_commit(document_id, revision)?;
    drop(editor_state);
    Ok((handle, lease, clear_history))
}

fn commit_current_file_save_without_reparse<F>(
    registry: &ActiveDocumentRepository,
    path: String,
    document_id: u64,
    revision: u64,
    output_name: String,
    saved_extension: String,
    commit_write: F,
) -> Result<SavedDocumentResponse, AppError>
where
    F: FnOnce() -> Result<(), AppError>,
{
    let (handle, lease, clear_history) =
        begin_prepared_save_commit(registry, document_id, revision, |editor_state| {
            let current_extension = extension_of(&editor_state.file_data().file_name)
                .or_else(|| extension_of(&editor_state.file_data().path));
            current_extension.as_deref() != Some(saved_extension.as_str())
        })?;

    if let Err(error) = commit_write() {
        abort_save_commit(&handle, lease);
        return Err(error);
    }

    let (response, retired) = {
        let mut editor_state = handle.write()?;
        let retired = editor_state.finish_save_commit_without_reparse(
            lease,
            path,
            output_name,
            clear_history,
        )?;
        let response = SavedDocumentResponse {
            document: None,
            identity: Some(SavedDocumentIdentity {
                path: editor_state.file_data().path.clone(),
                file_name: editor_state.file_data().file_name.clone(),
            }),
            editor_session: document_query_service::editor_session_info(&editor_state),
        };
        (response, retired)
    };
    drop(retired);
    Ok(response)
}

fn ensure_editor_matches_prepared_save(
    editor_state: &EditorState,
    document_id: u64,
    revision: u64,
) -> Result<(), AppError> {
    if editor_state.document_id() == document_id && editor_state.revision() == revision {
        return Ok(());
    }
    Err(AppError::DocumentStateInvalid(
        "document changed while save was being prepared; please save again".to_string(),
    ))
}

fn abort_save_commit(handle: &DocumentHandle, lease: SaveCommitLease) {
    if let Ok(mut editor_state) = handle.write() {
        editor_state.abort_save_commit(lease);
    }
}

fn document_handle_for_read(
    registry: &ActiveDocumentRepository,
    document_id: u64,
) -> Result<Arc<DocumentHandle>, AppError> {
    registry.read_handle(document_id)
}

fn current_document_path_for_command(
    runtime: &ApplicationRuntime,
    document_id: u64,
    base_revision: u64,
) -> Result<String, AppError> {
    let handle = document_handle_for_read(runtime.documents(), document_id)?;
    let editor_state = handle.read_for_command(document_id, base_revision)?;
    Ok(editor_state.file_data().path.clone())
}

fn default_extension_string() -> String {
    default_spreadsheet_extension().to_string()
}

#[cfg(desktop)]
pub fn save_file_desktop(
    runtime: &ApplicationRuntime,
    path: &str,
    document_id: u64,
    base_revision: u64,
) -> Result<SavedDocumentResponse, AppError> {
    use std::path::Path;

    use crate::io::atomic_file::{
        cleanup_temp_file, replace_temp_file, write_temp_file_for_target,
    };
    use crate::io::platform::desktop;

    let current_path = current_document_path_for_command(runtime, document_id, base_revision)?;
    desktop::ensure_save_path_authorized(runtime.desktop_files(), path, &current_path)?;
    let prepared = prepare_current_file_save(runtime, document_id, base_revision, path)?;
    let target = Path::new(path);
    let temp_path = match write_temp_file_for_target(target, &prepared.bytes) {
        Ok(temp_path) => temp_path,
        Err(error) => {
            abort_prepared_file_save(prepared);
            return Err(error);
        }
    };

    let result = commit_current_file_save(runtime, path.to_string(), prepared, || {
        replace_temp_file(&temp_path, target)
    });
    if result.is_err() {
        cleanup_temp_file(&temp_path);
    }
    result
}

#[cfg(desktop)]
pub fn export_file_desktop(
    runtime: &ApplicationRuntime,
    app: &tauri::AppHandle,
    default_name: &str,
    document_id: u64,
    base_revision: u64,
) -> Result<Option<String>, AppError> {
    use crate::io::platform::desktop;

    let Some(target) = desktop::pick_export_target(app, default_name)? else {
        return Ok(None);
    };
    let prepared = prepare_current_file_export(
        runtime,
        document_id,
        base_revision,
        &target.target_path_or_name,
    )?;
    desktop::write_export_target(&target, &prepared.bytes)?;
    Ok(Some(target.path_string))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn save_file_mobile(
    runtime: &ApplicationRuntime,
    app: &tauri::AppHandle,
    path: &str,
    document_id: u64,
    base_revision: u64,
) -> Result<SavedDocumentResponse, AppError> {
    use std::path::Path;

    use crate::io::atomic_file::{
        cleanup_temp_file, replace_temp_file, write_temp_file_for_target,
    };
    use crate::io::managed_documents;
    use crate::io::platform::mobile;

    let target = mobile::validated_mobile_files_path(runtime.mobile_files(), app, Path::new(path))?;
    let current_path = current_document_path_for_command(runtime, document_id, base_revision)?;
    mobile::ensure_save_target_authorized(runtime.mobile_files(), &target, &current_path)?;
    let target_path = target.to_string_lossy().to_string();
    let prepared = prepare_current_file_save(runtime, document_id, base_revision, &target_path)?;
    managed_documents::validate_managed_save(
        runtime.mobile_files().managed_documents(),
        &target,
        prepared.bytes.len() as u64,
    )?;
    let temp_path = match write_temp_file_for_target(&target, &prepared.bytes) {
        Ok(temp_path) => temp_path,
        Err(error) => {
            abort_prepared_file_save(prepared);
            return Err(error);
        }
    };

    let managed_file_name = prepared.output_name.clone();
    let result = commit_current_file_save(runtime, target_path, prepared, || {
        replace_temp_file(&temp_path, &target)?;
        managed_documents::adopt_completed_save(
            runtime.mobile_files().managed_documents(),
            runtime.mobile_files().transient_files(),
            &target,
            &managed_file_name,
        )
    });
    if result.is_err() {
        cleanup_temp_file(&temp_path);
    }
    result
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn export_file_mobile(
    runtime: &ApplicationRuntime,
    app: &tauri::AppHandle,
    default_name: &str,
    document_id: u64,
    base_revision: u64,
) -> Result<Option<String>, AppError> {
    use crate::io::platform::mobile;

    let Some(target) = mobile::pick_export_target(app, default_name)? else {
        return Ok(None);
    };
    let prepared = prepare_current_file_export(
        runtime,
        document_id,
        base_revision,
        &target.target_path_or_name,
    )?;
    mobile::write_export_target(app, &target, &prepared.bytes)?;
    Ok(Some(target.destination_string))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::editor_state::EditorState;
    use crate::types::{FileData, SheetData};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn test_registry() -> (ActiveDocumentRepository, u64, u64) {
        let state = EditorState::with_workbook(
            FileData {
                path: String::new(),
                file_name: "book.xlsx".to_string(),
                sheets: vec![SheetData::default()],
            },
            None,
        );
        let document_id = state.document_id();
        let revision = state.revision();
        let registry = ActiveDocumentRepository::default();
        registry.replace_active_for_test(state);
        (registry, document_id, revision)
    }

    fn prepared_for_test(document_id: u64, revision: u64) -> PreparedDocumentSave {
        PreparedDocumentSave {
            document_id,
            revision,
            output_name: "saved.xlsx".to_string(),
            bytes: Vec::new(),
            finish_without_reparse: true,
            _work: None,
        }
    }

    #[test]
    fn failed_write_releases_save_lease_without_changing_revision() {
        let (registry, document_id, revision) = test_registry();
        let search = SearchService::new();

        let error = commit_current_file_save_with_registry(
            &search,
            &registry,
            "/tmp/saved.xlsx".to_string(),
            prepared_for_test(document_id, revision),
            || Err(AppError::WriteError("injected write failure".to_string())),
        )
        .expect_err("write must fail");

        assert!(matches!(error, AppError::WriteError(_)));
        let handle = registry.active_handle().unwrap().unwrap();
        let state = handle.read().unwrap();
        assert_eq!(state.revision(), revision);
        assert!(!state.has_save_commit_in_progress());
    }

    #[test]
    fn successful_write_commits_identity_once() {
        let (registry, document_id, revision) = test_registry();
        let search = SearchService::new();
        let writes = AtomicUsize::new(0);

        let response = commit_current_file_save_with_registry(
            &search,
            &registry,
            "/tmp/saved.xlsx".to_string(),
            prepared_for_test(document_id, revision),
            || {
                writes.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .expect("save succeeds");

        assert_eq!(writes.load(Ordering::SeqCst), 1);
        assert_eq!(response.editor_session.revision, revision + 1);
        assert_eq!(response.identity.unwrap().file_name, "saved.xlsx");
    }

    #[test]
    fn stale_prepared_save_never_calls_writer() {
        let (registry, document_id, revision) = test_registry();
        let search = SearchService::new();
        let writes = AtomicUsize::new(0);

        let result = commit_current_file_save_with_registry(
            &search,
            &registry,
            "/tmp/saved.xlsx".to_string(),
            prepared_for_test(document_id, revision + 1),
            || {
                writes.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        );

        assert!(matches!(result, Err(AppError::DocumentStateInvalid(_))));
        assert_eq!(writes.load(Ordering::SeqCst), 0);
    }
}
