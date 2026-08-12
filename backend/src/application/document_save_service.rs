use std::sync::Arc;

use crate::application::document_codec_port::{DocumentCodecPort, SavedDocumentDecodePlan};
use crate::application::document_encode_port::DocumentEncodePort;
use crate::application::document_work_budget_port::{DocumentWorkBudgetPort, DocumentWorkLease};
use crate::application::search_ports::SearchIndexMaintenancePort;
use crate::application::{document_format_policy, document_projection};
use crate::document_format::{default_spreadsheet_extension, extension_of, is_xlsx_extension};
use crate::error::AppError;
use crate::projection_model::SavedDocumentOutcome;
use crate::resource_limits::{
    MAX_GENERATED_FILE_BYTES, validate_document_identity, validate_prepared_document_bytes,
};
use crate::state::{
    ActiveDocumentRepository, DocumentHandle,
    editor_state::{EditorState, SaveCommitLease},
};

#[derive(Clone)]
pub struct DocumentSaveService {
    documents: ActiveDocumentRepository,
    search_indexes: Arc<dyn SearchIndexMaintenancePort>,
    codec: Arc<dyn DocumentCodecPort>,
    encoder: Arc<dyn DocumentEncodePort>,
    work_budget: Arc<dyn DocumentWorkBudgetPort>,
}

impl DocumentSaveService {
    pub(crate) fn new(
        documents: ActiveDocumentRepository,
        search_indexes: Arc<dyn SearchIndexMaintenancePort>,
        codec: Arc<dyn DocumentCodecPort>,
        encoder: Arc<dyn DocumentEncodePort>,
        work_budget: Arc<dyn DocumentWorkBudgetPort>,
    ) -> Self {
        Self {
            documents,
            search_indexes,
            codec,
            encoder,
            work_budget,
        }
    }

    fn documents(&self) -> &ActiveDocumentRepository {
        &self.documents
    }

    fn search_indexes(&self) -> &dyn SearchIndexMaintenancePort {
        self.search_indexes.as_ref()
    }

    fn codec(&self) -> &dyn DocumentCodecPort {
        self.codec.as_ref()
    }

    fn encoder(&self) -> &dyn DocumentEncodePort {
        self.encoder.as_ref()
    }

    fn work_budget(&self) -> &dyn DocumentWorkBudgetPort {
        self.work_budget.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn is_isolated_from(&self, other: &Self) -> bool {
        !self.documents.is_same_instance(&other.documents)
            && !Arc::ptr_eq(&self.search_indexes, &other.search_indexes)
            && !Arc::ptr_eq(&self.codec, &other.codec)
            && !Arc::ptr_eq(&self.encoder, &other.encoder)
            && !Arc::ptr_eq(&self.work_budget, &other.work_budget)
    }
}

pub struct PreparedDocumentSave {
    pub document_id: u64,
    pub revision: u64,
    pub output_name: String,
    pub bytes: Vec<u8>,
    pub finish_without_reparse: bool,
    saved_decode_plan: Option<Box<dyn SavedDocumentDecodePlan>>,
    _work: Option<Box<dyn DocumentWorkLease>>,
}

pub struct PreparedDocumentExport {
    pub output_name: String,
    pub bytes: Vec<u8>,
}

struct SaveWithoutReparse {
    path: String,
    document_id: u64,
    revision: u64,
    output_name: String,
    saved_extension: String,
}

pub fn prepare_current_file_save(
    service: &DocumentSaveService,
    document_id: u64,
    base_revision: u64,
    target_path_or_name: &str,
) -> Result<PreparedDocumentSave, AppError> {
    let (snapshot, source_bytes, mut work) = {
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
        let source_bytes = editor_state.estimated_resource_bytes();
        let work = service
            .work_budget()
            .reserve_save(document_id, source_bytes, source_bytes)?;
        let snapshot = editor_state.save_snapshot_for_target(target_path_or_name)?;
        (snapshot, source_bytes, work)
    };

    work.set_work_bytes(source_bytes.saturating_add(MAX_GENERATED_FILE_BYTES))?;
    let (output_name, bytes) = service.encoder().encode(&snapshot, target_path_or_name)?;
    let target_extension = extension_of(&output_name)
        .or_else(|| extension_of(target_path_or_name))
        .unwrap_or_else(default_extension_string);
    let finish_without_reparse = is_xlsx_extension(&target_extension) && snapshot.is_excel_backed();
    let saved_decode_plan = if finish_without_reparse {
        work.set_work_bytes(source_bytes.saturating_add(bytes.len()))?;
        None
    } else {
        let plan = service.codec().plan_saved(&target_extension, &bytes)?;
        let estimated_parse_bytes = plan.estimated_parse_bytes();
        validate_prepared_document_bytes(estimated_parse_bytes)?;
        work.set_work_bytes(
            source_bytes
                .saturating_add(bytes.len())
                .saturating_add(estimated_parse_bytes),
        )?;
        Some(plan)
    };
    Ok(PreparedDocumentSave {
        document_id,
        revision: base_revision,
        output_name,
        bytes,
        finish_without_reparse,
        saved_decode_plan,
        _work: Some(work),
    })
}

pub fn prepare_current_file_export(
    service: &DocumentSaveService,
    document_id: u64,
    base_revision: u64,
    target_path_or_name: &str,
) -> Result<PreparedDocumentExport, AppError> {
    let (snapshot, source_bytes, mut work) = {
        let handle = document_handle_for_read(service.documents(), document_id)?;
        let editor_state = handle.read_for_command(document_id, base_revision)?;
        let source_bytes = editor_state.estimated_resource_bytes();
        let work = service
            .work_budget()
            .reserve_save(document_id, source_bytes, source_bytes)?;
        let snapshot = editor_state.save_snapshot_for_target(target_path_or_name)?;
        (snapshot, source_bytes, work)
    };

    work.set_work_bytes(source_bytes.saturating_add(MAX_GENERATED_FILE_BYTES))?;
    let (output_name, bytes) = service.encoder().encode(&snapshot, target_path_or_name)?;
    work.set_work_bytes(source_bytes.saturating_add(bytes.len()))?;
    Ok(PreparedDocumentExport { output_name, bytes })
}

pub fn commit_current_file_save_projected<T, F, P>(
    service: &DocumentSaveService,
    path: String,
    prepared: PreparedDocumentSave,
    commit_write: F,
    project: P,
) -> Result<T, AppError>
where
    F: FnOnce() -> Result<(), AppError>,
    P: FnOnce(SavedDocumentOutcome) -> Result<T, AppError>,
{
    commit_current_file_save_with_registry_projected(
        service.search_indexes(),
        service.documents(),
        path,
        prepared,
        commit_write,
        project,
    )
}

#[cfg(test)]
fn commit_current_file_save_with_registry<F>(
    search_indexes: &dyn SearchIndexMaintenancePort,
    registry: &ActiveDocumentRepository,
    path: String,
    prepared: PreparedDocumentSave,
    commit_write: F,
) -> Result<SavedDocumentOutcome, AppError>
where
    F: FnOnce() -> Result<(), AppError>,
{
    commit_current_file_save_with_registry_projected(
        search_indexes,
        registry,
        path,
        prepared,
        commit_write,
        Ok,
    )
}

fn commit_current_file_save_with_registry_projected<T, F, P>(
    search_indexes: &dyn SearchIndexMaintenancePort,
    registry: &ActiveDocumentRepository,
    path: String,
    prepared: PreparedDocumentSave,
    commit_write: F,
    project: P,
) -> Result<T, AppError>
where
    F: FnOnce() -> Result<(), AppError>,
    P: FnOnce(SavedDocumentOutcome) -> Result<T, AppError>,
{
    let PreparedDocumentSave {
        document_id,
        revision,
        output_name,
        bytes,
        finish_without_reparse,
        saved_decode_plan,
        _work,
    } = prepared;
    let extension = extension_of(&output_name)
        .or_else(|| extension_of(&path))
        .unwrap_or_else(default_extension_string);
    validate_document_identity(&path, &output_name)?;
    if finish_without_reparse {
        return commit_current_file_save_without_reparse(
            search_indexes,
            registry,
            SaveWithoutReparse {
                path,
                document_id,
                revision,
                output_name,
                saved_extension: extension,
            },
            commit_write,
            project,
        );
    }

    let decode_plan = saved_decode_plan.ok_or_else(|| {
        AppError::Internal("prepared save is missing its decode plan".to_string())
    })?;
    let document = decode_plan.decode(bytes, path, output_name)?;
    let saved_extension = extension_of(&document.projection().file_name)
        .or_else(|| extension_of(&document.projection().path));
    let (handle, lease, clear_history) =
        begin_prepared_save_commit(registry, document_id, revision, |editor_state| {
            let current_extension = extension_of(&editor_state.file_data().file_name)
                .or_else(|| extension_of(&editor_state.file_data().path));
            current_extension != saved_extension
        })?;

    let projected = match (|| {
        let editor_state = handle.read()?;
        let response = document_projection::saved_document_outcome_with_reparse(
            &editor_state,
            &document,
            clear_history,
        )?;
        project(response)
    })() {
        Ok(projected) => projected,
        Err(error) => {
            abort_save_commit(&handle, lease);
            return Err(error);
        }
    };

    if let Err(error) = commit_write() {
        abort_save_commit(&handle, lease);
        return Err(error);
    }

    let (document_id, retired) = {
        let mut editor_state = handle.write()?;
        let retired = editor_state.finish_save_commit(lease, document, clear_history)?;
        (editor_state.document_id(), retired)
    };
    drop(retired);
    search_indexes.rebuild_all_sheets_index(document_id);
    Ok(projected)
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

fn commit_current_file_save_without_reparse<T, F, P>(
    search_indexes: &dyn SearchIndexMaintenancePort,
    registry: &ActiveDocumentRepository,
    save: SaveWithoutReparse,
    commit_write: F,
    project: P,
) -> Result<T, AppError>
where
    F: FnOnce() -> Result<(), AppError>,
    P: FnOnce(SavedDocumentOutcome) -> Result<T, AppError>,
{
    let SaveWithoutReparse {
        path,
        document_id,
        revision,
        output_name,
        saved_extension,
    } = save;
    let (handle, lease, clear_history) =
        begin_prepared_save_commit(registry, document_id, revision, |editor_state| {
            let current_extension = extension_of(&editor_state.file_data().file_name)
                .or_else(|| extension_of(&editor_state.file_data().path));
            current_extension.as_deref() != Some(saved_extension.as_str())
        })?;

    let projected = match (|| {
        let editor_state = handle.read()?;
        let response = document_projection::saved_document_outcome_without_reparse(
            &editor_state,
            path.clone(),
            output_name.clone(),
            clear_history,
        )?;
        project(response)
    })() {
        Ok(projected) => projected,
        Err(error) => {
            abort_save_commit(&handle, lease);
            return Err(error);
        }
    };

    if let Err(error) = commit_write() {
        abort_save_commit(&handle, lease);
        return Err(error);
    }

    let (revision, retired) = {
        let mut editor_state = handle.write()?;
        let retired = editor_state.finish_save_commit_without_reparse(
            lease,
            path,
            output_name,
            clear_history,
        )?;
        (editor_state.revision(), retired)
    };
    drop(retired);
    search_indexes.schedule_work(document_id, revision, crate::domain::SearchIndexWork::None);
    Ok(projected)
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

fn default_extension_string() -> String {
    default_spreadsheet_extension().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::search_ports::{
        NoopSearchIndexMaintenancePort, SearchIndexMaintenancePort,
    };
    use crate::document_data::{DocumentData, DocumentSheet};
    use crate::domain::SearchIndexWork;
    use crate::state::editor_state::EditorState;

    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct RecordingSearchPort {
        scheduled: Mutex<Vec<(u64, u64, SearchIndexWork)>>,
    }

    impl SearchIndexMaintenancePort for RecordingSearchPort {
        fn rebuild_all_sheets_index(&self, _document_id: u64) {}

        fn schedule_work(&self, document_id: u64, source_revision: u64, work: SearchIndexWork) {
            self.scheduled
                .lock()
                .unwrap()
                .push((document_id, source_revision, work));
        }

        fn cancel_document_jobs(&self, _document_id: u64) {}
    }

    fn test_registry() -> (ActiveDocumentRepository, u64, u64) {
        let state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "book.xlsx".to_string(),
                sheets: vec![DocumentSheet::default()],
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
            saved_decode_plan: None,
            _work: None,
        }
    }

    #[test]
    fn failed_write_releases_save_lease_without_changing_revision() {
        let (registry, document_id, revision) = test_registry();
        let search_indexes = NoopSearchIndexMaintenancePort;

        let error = commit_current_file_save_with_registry(
            &search_indexes,
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
        let search_port = Arc::new(RecordingSearchPort::default());
        let writes = AtomicUsize::new(0);

        let response = commit_current_file_save_with_registry(
            search_port.as_ref(),
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
        assert_eq!(
            *search_port.scheduled.lock().unwrap(),
            vec![(document_id, revision + 1, SearchIndexWork::None)]
        );
    }

    #[test]
    fn stale_prepared_save_never_calls_writer() {
        let (registry, document_id, revision) = test_registry();
        let search_indexes = NoopSearchIndexMaintenancePort;
        let writes = AtomicUsize::new(0);

        let result = commit_current_file_save_with_registry(
            &search_indexes,
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

    #[test]
    fn failed_response_projection_never_calls_writer_or_changes_state() {
        let (registry, document_id, revision) = test_registry();
        let search_indexes = NoopSearchIndexMaintenancePort;
        let writes = AtomicUsize::new(0);

        let result = commit_current_file_save_with_registry_projected(
            &search_indexes,
            &registry,
            "/tmp/saved.xlsx".to_string(),
            prepared_for_test(document_id, revision),
            || {
                writes.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
            |_| -> Result<(), AppError> {
                Err(AppError::ResourceLimitExceeded(
                    "injected response admission failure".to_string(),
                ))
            },
        );

        assert!(matches!(result, Err(AppError::ResourceLimitExceeded(_))));
        assert_eq!(writes.load(Ordering::SeqCst), 0);
        let handle = registry.active_handle().unwrap().unwrap();
        let state = handle.read().unwrap();
        assert_eq!(state.revision(), revision);
        assert_eq!(state.file_data().file_name, "book.xlsx");
        assert!(!state.has_save_commit_in_progress());
    }
}
