use std::sync::Arc;

use crate::application::document_codec_port::DocumentCodecPort;
use crate::application::document_encode_port::DocumentEncodePort;
use crate::application::document_work_budget_port::{DocumentWorkBudgetPort, DocumentWorkLease};
use crate::application::search_ports::SearchIndexMaintenancePort;
use crate::application::{document_format_policy, document_projection};
use crate::document_format::{default_spreadsheet_extension, extension_of, is_xlsx_extension};
use crate::error::AppError;
use crate::projection_model::{SavedDocumentIdentity, SavedDocumentOutcome};
use crate::state::{
    editor_state::{EditorState, SaveCommitLease},
    state::{ActiveDocumentRepository, DocumentHandle},
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

pub struct PreparedDocumentExport {
    pub bytes: Vec<u8>,
    _work: Option<Box<dyn DocumentWorkLease>>,
}

pub struct PreparedDocumentSave {
    pub document_id: u64,
    pub revision: u64,
    pub output_name: String,
    pub bytes: Vec<u8>,
    pub finish_without_reparse: bool,
    _work: Option<Box<dyn DocumentWorkLease>>,
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
            .work_budget()
            .reserve_save(document_id, editor_state.estimated_resource_bytes())?;
        let snapshot = editor_state.save_snapshot_for_target(target_path_or_name)?;
        (snapshot, work)
    };
    let (_, bytes) = service.encoder().encode(&snapshot, target_path_or_name)?;
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
            .work_budget()
            .reserve_save(document_id, editor_state.estimated_resource_bytes())?;
        let snapshot = editor_state.save_snapshot_for_target(target_path_or_name)?;
        (snapshot, work)
    };

    let (output_name, bytes) = service.encoder().encode(&snapshot, target_path_or_name)?;
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
) -> Result<SavedDocumentOutcome, AppError>
where
    F: FnOnce() -> Result<(), AppError>,
{
    commit_current_file_save_with_registry(
        service.search_indexes(),
        service.documents(),
        service.codec(),
        path,
        prepared,
        commit_write,
    )
}

fn commit_current_file_save_with_registry<F>(
    search_indexes: &dyn SearchIndexMaintenancePort,
    registry: &ActiveDocumentRepository,
    codec: &dyn DocumentCodecPort,
    path: String,
    prepared: PreparedDocumentSave,
    commit_write: F,
) -> Result<SavedDocumentOutcome, AppError>
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
            search_indexes,
            registry,
            path,
            document_id,
            revision,
            output_name,
            extension,
            commit_write,
        );
    }

    let document = codec.decode_saved(&extension, bytes, path.clone(), output_name)?;
    let saved_extension = extension_of(&document.projection().file_name)
        .or_else(|| extension_of(&document.projection().path));
    let (handle, lease, clear_history) =
        begin_prepared_save_commit(registry, document_id, revision, |editor_state| {
            let current_extension = extension_of(&editor_state.file_data().file_name)
                .or_else(|| extension_of(&editor_state.file_data().path));
            current_extension != saved_extension
        })?;

    if let Err(error) = commit_write() {
        abort_save_commit(&handle, lease);
        return Err(error);
    }

    let (document_id, response, retired) = {
        let mut editor_state = handle.write()?;
        let retired = editor_state.finish_save_commit(lease, document, clear_history)?;
        let response = SavedDocumentOutcome {
            document: Some(document_projection::document_manifest(&editor_state)),
            identity: None,
            editor_session: document_projection::editor_session_snapshot(&editor_state),
        };
        (editor_state.document_id(), response, retired)
    };
    drop(retired);
    search_indexes.rebuild_all_sheets_index(document_id);
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
    search_indexes: &dyn SearchIndexMaintenancePort,
    registry: &ActiveDocumentRepository,
    path: String,
    document_id: u64,
    revision: u64,
    output_name: String,
    saved_extension: String,
    commit_write: F,
) -> Result<SavedDocumentOutcome, AppError>
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
        let response = SavedDocumentOutcome {
            document: None,
            identity: Some(SavedDocumentIdentity {
                path: editor_state.file_data().path.clone(),
                file_name: editor_state.file_data().file_name.clone(),
            }),
            editor_session: document_projection::editor_session_snapshot(&editor_state),
        };
        (response, retired)
    };
    drop(retired);
    search_indexes.schedule_work(
        document_id,
        response.editor_session.revision,
        crate::domain::SearchIndexWork::None,
    );
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
    use crate::application::search_ports::{
        NoopSearchIndexMaintenancePort, SearchIndexMaintenancePort,
    };
    use crate::document_data::{DocumentData, DocumentSheet};
    use crate::domain::SearchIndexWork;
    use crate::state::editor_state::EditorState;

    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestCodec;

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

    impl DocumentCodecPort for TestCodec {
        fn plan_open(
            &self,
            _source: &crate::application::document_codec_port::OpenDocumentSource,
        ) -> Result<Box<dyn crate::application::document_codec_port::DocumentDecodePlan>, AppError>
        {
            unreachable!("save-service tests do not open files")
        }

        fn decode_saved(
            &self,
            _extension: &str,
            _bytes: Vec<u8>,
            _path: String,
            _file_name: String,
        ) -> Result<crate::document::document_model::SpreadsheetDocument, AppError> {
            unreachable!("native XLSX test saves do not require reparsing")
        }
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
            &TestCodec,
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
            &TestCodec,
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
            &TestCodec,
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
