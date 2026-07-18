use std::sync::Arc;

use crate::application::search_service::SearchService;
use crate::application::{document_format_policy, document_projection};
use crate::error::AppError;
use crate::io::codec::reader::read_file_with_workbook_from_bytes;
use crate::io::file_format::{default_spreadsheet_extension, extension_of, is_xlsx_extension};
use crate::io::save_work::{SaveWorkCoordinator, SaveWorkReservation};
use crate::state::{
    editor_state::{EditorState, SaveCommitLease},
    state::{ActiveDocumentRepository, DocumentHandle},
};
use crate::types::{SavedDocumentIdentity, SavedDocumentResponse};

#[derive(Clone)]
pub struct DocumentSaveService {
    documents: ActiveDocumentRepository,
    search: SearchService,
    save_work: SaveWorkCoordinator,
}

impl DocumentSaveService {
    pub(crate) fn new(
        documents: ActiveDocumentRepository,
        search: SearchService,
        save_work: SaveWorkCoordinator,
    ) -> Self {
        Self {
            documents,
            search,
            save_work,
        }
    }

    fn documents(&self) -> &ActiveDocumentRepository {
        &self.documents
    }

    fn search(&self) -> &SearchService {
        &self.search
    }

    fn save_work(&self) -> &SaveWorkCoordinator {
        &self.save_work
    }

    #[cfg(test)]
    pub(crate) fn is_isolated_from(&self, other: &Self) -> bool {
        !self.documents.is_same_instance(&other.documents)
            && !self.save_work.is_same_instance(&other.save_work)
            && self.search.is_isolated_from(&other.search)
    }
}

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
    service: &DocumentSaveService,
    document_id: u64,
    base_revision: u64,
    target_path_or_name: &str,
) -> Result<PreparedDocumentExport, AppError> {
    let handle = document_handle_for_read(service.documents(), document_id)?;
    let (snapshot, work) = {
        let editor_state = handle.read_for_command(document_id, base_revision)?;
        let work = service
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
    service: &DocumentSaveService,
    document_id: u64,
    base_revision: u64,
    target_path_or_name: &str,
) -> Result<PreparedDocumentSave, AppError> {
    let (snapshot, work) = {
        let handle = document_handle_for_read(service.documents(), document_id)?;
        let editor_state = handle.read_for_command(document_id, base_revision)?;
        document_format_policy::ensure_native_save_target_allowed(
            &editor_state,
            target_path_or_name,
        )?;
        if editor_state.has_save_commit_in_progress() {
            return Err(AppError::DocumentStateInvalid(
                "save is already in progress".to_string(),
            ));
        }
        let work = service
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
    service: &DocumentSaveService,
    path: String,
    prepared: PreparedDocumentSave,
    commit_write: F,
) -> Result<SavedDocumentResponse, AppError>
where
    F: FnOnce() -> Result<(), AppError>,
{
    commit_current_file_save_with_registry(
        service.search(),
        service.documents(),
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
            document: Some(document_projection::document_manifest(&editor_state)),
            identity: None,
            editor_session: document_projection::editor_session_info(&editor_state),
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
            editor_session: document_projection::editor_session_info(&editor_state),
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

pub(crate) fn current_document_path_for_command(
    service: &DocumentSaveService,
    document_id: u64,
    base_revision: u64,
) -> Result<String, AppError> {
    let handle = document_handle_for_read(service.documents(), document_id)?;
    let editor_state = handle.read_for_command(document_id, base_revision)?;
    Ok(editor_state.file_data().path.clone())
}

fn default_extension_string() -> String {
    default_spreadsheet_extension().to_string()
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
