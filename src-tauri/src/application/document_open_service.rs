use crate::document_data::{DocumentData, DocumentSheet};
use std::path::PathBuf;
use std::sync::Arc;

use crate::application::document_codec_port::{DocumentCodecPort, OpenDocumentSource};
use crate::application::document_projection;
use crate::application::document_work_budget_port::{DocumentWorkBudgetPort, DocumentWorkLease};
use crate::application::prepared_document_repository::{
    self, PrepareReservation, PreparedDocumentFingerprint, PreparedDocumentRepository,
};
use crate::document_format::default_spreadsheet_extension;
use crate::error::AppError;
use crate::projection_model::PreparedOpenDocument;
use crate::resource_limits::{
    ResourceLedger, validate_active_and_prepared_document_bytes, validate_file_data,
    validate_prepared_document_bytes,
};
use crate::state::editor_state::EditorState;
use crate::state::state::ActiveDocumentRepository;

#[derive(Clone)]
pub struct DocumentOpenService {
    documents: ActiveDocumentRepository,
    prepared_documents: PreparedDocumentRepository,
    codec: Arc<dyn DocumentCodecPort>,
    work_budget: Arc<dyn DocumentWorkBudgetPort>,
}

impl DocumentOpenService {
    pub(crate) fn new(
        documents: ActiveDocumentRepository,
        prepared_documents: PreparedDocumentRepository,
        codec: Arc<dyn DocumentCodecPort>,
        work_budget: Arc<dyn DocumentWorkBudgetPort>,
    ) -> Self {
        Self {
            documents,
            prepared_documents,
            codec,
            work_budget,
        }
    }

    fn documents(&self) -> &ActiveDocumentRepository {
        &self.documents
    }

    pub(crate) fn prepared_documents(&self) -> &PreparedDocumentRepository {
        &self.prepared_documents
    }

    fn codec(&self) -> &dyn DocumentCodecPort {
        self.codec.as_ref()
    }

    fn work_budget(&self) -> &dyn DocumentWorkBudgetPort {
        self.work_budget.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn is_isolated_from(&self, other: &Self) -> bool {
        !self.documents.is_same_instance(&other.documents)
            && !self
                .prepared_documents
                .is_same_instance(&other.prepared_documents)
            && !Arc::ptr_eq(&self.codec, &other.codec)
            && !Arc::ptr_eq(&self.work_budget, &other.work_budget)
    }
}

pub fn prepare_open_input(
    service: &DocumentOpenService,
    source: OpenDocumentSource,
    reservation: PrepareReservation,
) -> Result<PreparedOpenDocument, AppError> {
    let source_path = PathBuf::from(&source.path);
    let plan = service.codec().plan_open(&source)?;
    let estimated_parse_bytes = plan.estimated_parse_bytes();
    let active_document_bytes = active_document_resource_bytes(service)?;
    validate_prepared_document_bytes(estimated_parse_bytes)?;
    validate_active_and_prepared_document_bytes(active_document_bytes, estimated_parse_bytes)?;
    let mut work = service
        .work_budget()
        .reserve_preparation(active_document_bytes, estimated_parse_bytes)?;
    let editor_state = plan
        .decode(source)?
        .with_resource_estimate_floor(estimated_parse_bytes);
    work.set_work_bytes(editor_state.estimated_resource_bytes())?;

    prepare_editor_state(service, editor_state, Some(source_path), reservation, work)
}

pub fn prepare_new_file(
    service: &DocumentOpenService,
    preparation_id: &str,
) -> Result<PreparedOpenDocument, AppError> {
    let fingerprint = PreparedDocumentFingerprint::new_file();
    let reservation = match service
        .prepared_documents()
        .reserve(preparation_id, fingerprint)?
    {
        prepared_document_repository::PrepareReservationResult::Execute(reservation) => reservation,
        prepared_document_repository::PrepareReservationResult::Replay => {
            return replay_prepared_document(service, preparation_id, fingerprint);
        }
    };
    let file_data = blank_file_data();
    validate_file_data(&file_data)?;
    let active_document_bytes = active_document_resource_bytes(service)?;
    let estimated_prepared_bytes = ResourceLedger::from_file_data(&file_data)
        .estimated_bytes()
        .saturating_mul(2);
    validate_prepared_document_bytes(estimated_prepared_bytes)?;
    validate_active_and_prepared_document_bytes(active_document_bytes, estimated_prepared_bytes)?;
    let mut work = service
        .work_budget()
        .reserve_preparation(active_document_bytes, estimated_prepared_bytes)?;
    let editor_state = service.codec().create_document(file_data)?;
    work.set_work_bytes(editor_state.estimated_resource_bytes())?;
    prepare_editor_state(service, editor_state, None, reservation, work)
}

pub(crate) fn replay_prepared_document(
    service: &DocumentOpenService,
    preparation_id: &str,
    fingerprint: PreparedDocumentFingerprint,
) -> Result<PreparedOpenDocument, AppError> {
    service
        .prepared_documents()
        .project(preparation_id, fingerprint, |editor_state| {
            PreparedOpenDocument {
                token: preparation_id.to_string(),
                preview: document_projection::open_document_snapshot(editor_state),
            }
        })
}

pub fn abort_prepared_document(service: &DocumentOpenService, token: &str) -> Result<(), AppError> {
    service.prepared_documents().abort(token)
}

fn blank_file_data() -> DocumentData {
    DocumentData {
        path: String::new(),
        file_name: format!("untitled.{}", default_spreadsheet_extension()),
        sheets: vec![DocumentSheet {
            name: "Sheet1".to_string(),
            rows: vec![vec![crate::domain::CellValue::Null; 5]; 5],
            ..Default::default()
        }],
    }
}

fn prepare_editor_state(
    service: &DocumentOpenService,
    editor_state: EditorState,
    source_path: Option<PathBuf>,
    reservation: prepared_document_repository::PrepareReservation,
    work: Box<dyn DocumentWorkLease>,
) -> Result<PreparedOpenDocument, AppError> {
    let preview = document_projection::open_document_snapshot(&editor_state);
    let token = service.prepared_documents().replace(
        editor_state,
        source_path,
        work,
        reservation,
        active_document_resource_bytes(service)?,
    )?;
    Ok(PreparedOpenDocument { token, preview })
}

fn active_document_resource_bytes(service: &DocumentOpenService) -> Result<usize, AppError> {
    let handle = service.documents().active_handle()?;
    handle
        .map(|handle| handle.read().map(|state| state.estimated_resource_bytes()))
        .transpose()
        .map(|bytes| bytes.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::CellValue;

    struct TestCodec;
    struct TestDecodePlan;
    struct TestWorkBudget;
    struct TestWorkLease;

    impl DocumentWorkLease for TestWorkLease {
        fn set_work_bytes(&mut self, _work_bytes: usize) -> Result<(), AppError> {
            Ok(())
        }
    }

    impl DocumentWorkBudgetPort for TestWorkBudget {
        fn reserve_preparation(
            &self,
            _active_document_bytes: usize,
            _estimated_work_bytes: usize,
        ) -> Result<Box<dyn DocumentWorkLease>, AppError> {
            Ok(Box::new(TestWorkLease))
        }

        fn reserve_save(
            &self,
            _document_id: u64,
            _active_document_bytes: usize,
            _estimated_source_bytes: usize,
        ) -> Result<Box<dyn DocumentWorkLease>, AppError> {
            unreachable!("open-service tests do not save files")
        }
    }

    impl crate::application::document_codec_port::DocumentDecodePlan for TestDecodePlan {
        fn estimated_parse_bytes(&self) -> usize {
            2 * 1024 * 1024
        }

        fn decode(self: Box<Self>, source: OpenDocumentSource) -> Result<EditorState, AppError> {
            let text = String::from_utf8(source.bytes)
                .map_err(|error| AppError::ReadError(error.to_string()))?;
            Ok(EditorState::with_workbook(
                DocumentData {
                    path: source.path,
                    file_name: source
                        .file_name
                        .unwrap_or_else(|| "unknown.csv".to_string()),
                    sheets: vec![DocumentSheet {
                        rows: vec![vec![CellValue::String(text)]],
                        ..Default::default()
                    }],
                },
                None,
            ))
        }
    }

    impl DocumentCodecPort for TestCodec {
        fn plan_open(
            &self,
            _source: &OpenDocumentSource,
        ) -> Result<Box<dyn crate::application::document_codec_port::DocumentDecodePlan>, AppError>
        {
            Ok(Box::new(TestDecodePlan))
        }

        fn plan_saved(
            &self,
            _extension: &str,
            _bytes: &[u8],
        ) -> Result<
            Box<dyn crate::application::document_codec_port::SavedDocumentDecodePlan>,
            AppError,
        > {
            unreachable!("open-service tests do not reparse saved files")
        }
    }

    fn service() -> DocumentOpenService {
        DocumentOpenService::new(
            ActiveDocumentRepository::default(),
            PreparedDocumentRepository::default(),
            Arc::new(TestCodec),
            Arc::new(TestWorkBudget),
        )
    }

    #[test]
    fn open_input_is_decoded_through_the_injected_codec() {
        let service = service();
        let fingerprint = PreparedDocumentFingerprint::open("/tmp/imported");
        let reservation = match service
            .prepared_documents()
            .reserve("prepare-open", fingerprint)
            .expect("reserve preparation")
        {
            prepared_document_repository::PrepareReservationResult::Execute(reservation) => {
                reservation
            }
            prepared_document_repository::PrepareReservationResult::Replay => {
                panic!("first preparation cannot replay")
            }
        };
        let prepared = prepare_open_input(
            &service,
            OpenDocumentSource {
                path: "/tmp/imported".to_string(),
                bytes: b"decoded through port".to_vec(),
                file_name: Some("imported.csv".to_string()),
            },
            reservation,
        )
        .expect("prepare source");
        let response = service
            .prepared_documents()
            .take(&prepared.token)
            .expect("prepared document");

        assert_eq!(
            response.editor_state.file_data().sheets[0].rows[0][0],
            CellValue::String("decoded through port".to_string())
        );
        assert!(response.editor_state.estimated_resource_bytes() >= 2 * 1024 * 1024);
    }

    #[test]
    fn new_file_uses_the_backend_owned_blank_template() {
        let service = service();
        let prepared = prepare_new_file(&service, "prepare-new").expect("init file");
        let response = service
            .prepared_documents()
            .take(&prepared.token)
            .expect("prepared document");

        assert_eq!(response.editor_state.file_data().path, "");
        assert_eq!(response.editor_state.file_data().file_name, "untitled.xlsx");
        assert_eq!(response.editor_state.file_data().sheets.len(), 1);
        assert_eq!(response.editor_state.file_data().sheets[0].name, "Sheet1");
        assert_eq!(response.editor_state.file_data().sheets[0].rows.len(), 5);
        assert!(
            response.editor_state.file_data().sheets[0]
                .rows
                .iter()
                .all(|row| row.len() == 5 && row.iter().all(|cell| cell == &CellValue::Null))
        );
    }

    #[test]
    fn retrying_a_completed_preparation_id_replays_the_same_document() {
        let service = service();

        let first = prepare_new_file(&service, "retryable-new").expect("first preparation");
        let replayed = prepare_new_file(&service, "retryable-new").expect("replayed preparation");

        assert_eq!(first.token, replayed.token);
        assert_eq!(
            first.preview.document.file_name,
            replayed.preview.document.file_name
        );
        service
            .prepared_documents()
            .take(&first.token)
            .expect("one prepared document remains");
        assert!(service.prepared_documents().take(&first.token).is_err());
    }
}
