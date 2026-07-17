use std::sync::{Arc, RwLock};

use crate::error::AppError;
use crate::io::codec::reader::read_file_with_workbook_from_bytes;
use crate::io::document;
use crate::io::file_format::{default_spreadsheet_extension, extension_of, is_xlsx_extension};
use crate::io::save_work::{self, SaveWorkReservation};
use crate::ops::index_ops::spawn_rebuild_all_sheets_index;
use crate::state::{
    active_document_store,
    editor_state::{EditorState, SaveCommitLease},
    state::{ActiveDocumentStore, DocumentHandle},
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
    document_id: u64,
    base_revision: u64,
    target_path_or_name: &str,
) -> Result<PreparedDocumentExport, AppError> {
    let registry = active_document_store();
    let handle = document_handle_for_read(&registry, document_id)?;
    let (snapshot, work) = {
        let editor_state = handle.read_for_command(document_id, base_revision)?;
        let work = save_work::reserve(document_id, editor_state.estimated_resource_bytes())?;
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
    document_id: u64,
    base_revision: u64,
    target_path_or_name: &str,
) -> Result<PreparedDocumentSave, AppError> {
    let registry = active_document_store();
    let (snapshot, work) = {
        let handle = document_handle_for_read(&registry, document_id)?;
        let editor_state = handle.read_for_command(document_id, base_revision)?;
        document::ensure_native_save_target_allowed(&editor_state, target_path_or_name)?;
        if editor_state.has_save_commit_in_progress() {
            return Err(AppError::DocumentStateInvalid(
                "save is already in progress".to_string(),
            ));
        }
        let work = save_work::reserve(document_id, editor_state.estimated_resource_bytes())?;
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
    path: String,
    prepared: PreparedDocumentSave,
    commit_write: F,
) -> Result<SavedDocumentResponse, AppError>
where
    F: FnOnce() -> Result<(), AppError>,
{
    let registry = active_document_store();
    commit_current_file_save_with_registry(&registry, path, prepared, commit_write)
}

fn commit_current_file_save_with_registry<F>(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
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
            document: Some(document::document_manifest(&editor_state)),
            identity: None,
            editor_session: document::editor_session_info(&editor_state),
        };
        (editor_state.document_id(), response, retired)
    };
    drop(retired);
    spawn_rebuild_all_sheets_index(registry, document_id);
    Ok(response)
}

fn begin_prepared_save_commit<F>(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
    revision: u64,
    clear_history: F,
) -> Result<(Arc<DocumentHandle>, SaveCommitLease, bool), AppError>
where
    F: FnOnce(&EditorState) -> bool,
{
    let registry_guard = registry
        .read()
        .map_err(|_| AppError::poisoned_lock("document registry"))?;
    let handle = registry_guard.active_handle_for_mutation(document_id)?;
    let mut editor_state = handle.write()?;
    ensure_editor_matches_prepared_save(&editor_state, document_id, revision)?;
    let clear_history = clear_history(&editor_state);
    let lease = editor_state.begin_save_commit(document_id, revision)?;
    drop(editor_state);
    drop(registry_guard);
    Ok((handle, lease, clear_history))
}

fn commit_current_file_save_without_reparse<F>(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
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
            editor_session: document::editor_session_info(&editor_state),
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
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
) -> Result<Arc<DocumentHandle>, AppError> {
    registry
        .read()
        .map_err(|_| AppError::poisoned_lock("document registry"))?
        .active_handle_for_read(document_id)
}

fn default_extension_string() -> String {
    default_spreadsheet_extension().to_string()
}

#[cfg(desktop)]
pub fn save_file_desktop(
    path: &str,
    document_id: u64,
    base_revision: u64,
) -> Result<SavedDocumentResponse, AppError> {
    use std::path::Path;

    use crate::io::atomic_file::{
        cleanup_temp_file, replace_temp_file, write_temp_file_for_target,
    };
    use crate::io::platform::desktop;

    desktop::ensure_save_path_authorized(path, document_id, base_revision)?;
    let prepared = prepare_current_file_save(document_id, base_revision, path)?;
    let target = Path::new(path);
    let temp_path = match write_temp_file_for_target(target, &prepared.bytes) {
        Ok(temp_path) => temp_path,
        Err(error) => {
            abort_prepared_file_save(prepared);
            return Err(error);
        }
    };

    let result = commit_current_file_save(path.to_string(), prepared, || {
        replace_temp_file(&temp_path, target)
    });
    if result.is_err() {
        cleanup_temp_file(&temp_path);
    }
    result
}

#[cfg(desktop)]
pub fn export_file_desktop(
    app: &tauri::AppHandle,
    default_name: &str,
    document_id: u64,
    base_revision: u64,
) -> Result<Option<String>, AppError> {
    use crate::io::platform::desktop;

    let Some(target) = desktop::pick_export_target(app, default_name)? else {
        return Ok(None);
    };
    let prepared =
        prepare_current_file_export(document_id, base_revision, &target.target_path_or_name)?;
    desktop::write_export_target(&target, &prepared.bytes)?;
    Ok(Some(target.path_string))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn save_file_mobile(
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

    let target = mobile::validated_mobile_files_path(app, Path::new(path))?;
    mobile::ensure_save_target_authorized(&target, document_id, base_revision)?;
    let target_path = target.to_string_lossy().to_string();
    let prepared = prepare_current_file_save(document_id, base_revision, &target_path)?;
    managed_documents::validate_managed_save(&target, prepared.bytes.len() as u64)?;
    let temp_path = match write_temp_file_for_target(&target, &prepared.bytes) {
        Ok(temp_path) => temp_path,
        Err(error) => {
            abort_prepared_file_save(prepared);
            return Err(error);
        }
    };

    let managed_file_name = prepared.output_name.clone();
    let result = commit_current_file_save(target_path, prepared, || {
        replace_temp_file(&temp_path, &target)?;
        managed_documents::adopt_completed_save(&target, &managed_file_name)
    });
    if result.is_err() {
        cleanup_temp_file(&temp_path);
    }
    result
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn export_file_mobile(
    app: &tauri::AppHandle,
    default_name: &str,
    document_id: u64,
    base_revision: u64,
) -> Result<Option<String>, AppError> {
    use crate::io::platform::mobile;

    let Some(target) = mobile::pick_export_target(app, default_name)? else {
        return Ok(None);
    };
    let prepared =
        prepare_current_file_export(document_id, base_revision, &target.target_path_or_name)?;
    mobile::write_export_target(app, &target, &prepared.bytes)?;
    Ok(Some(target.destination_string))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::editor_state::EditorState;
    use crate::types::{FileData, SheetData};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn test_registry() -> (Arc<RwLock<ActiveDocumentStore>>, u64, u64) {
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
        let mut store = ActiveDocumentStore::new_for_test();
        store.replace_active_for_test(state);
        (Arc::new(RwLock::new(store)), document_id, revision)
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

        let error = commit_current_file_save_with_registry(
            &registry,
            "/tmp/saved.xlsx".to_string(),
            prepared_for_test(document_id, revision),
            || Err(AppError::WriteError("injected write failure".to_string())),
        )
        .expect_err("write must fail");

        assert!(matches!(error, AppError::WriteError(_)));
        let handle = registry.read().unwrap().active_handle().unwrap();
        let state = handle.read().unwrap();
        assert_eq!(state.revision(), revision);
        assert!(!state.has_save_commit_in_progress());
    }

    #[test]
    fn successful_write_commits_identity_once() {
        let (registry, document_id, revision) = test_registry();
        let writes = AtomicUsize::new(0);

        let response = commit_current_file_save_with_registry(
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
        let writes = AtomicUsize::new(0);

        let result = commit_current_file_save_with_registry(
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
