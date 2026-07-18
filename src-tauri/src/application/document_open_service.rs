use std::path::PathBuf;
use std::sync::Arc;

use crate::application::document_codec_port::{DocumentCodecPort, OpenDocumentSource};
use crate::application::prepared_document_repository::{self, PreparedDocumentRepository};
use crate::document_format::default_spreadsheet_extension;
use crate::error::AppError;
use crate::resource_limits::validate_file_data;
use crate::state::editor_state::EditorState;
use crate::state::state::ActiveDocumentRepository;
use crate::types::{FileData, PreparedOpenDocument, SheetData};

#[derive(Clone)]
pub struct DocumentOpenService {
    documents: ActiveDocumentRepository,
    prepared_documents: PreparedDocumentRepository,
    codec: Arc<dyn DocumentCodecPort>,
}

impl DocumentOpenService {
    pub(crate) fn new(
        documents: ActiveDocumentRepository,
        prepared_documents: PreparedDocumentRepository,
        codec: Arc<dyn DocumentCodecPort>,
    ) -> Self {
        Self {
            documents,
            prepared_documents,
            codec,
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

    #[cfg(test)]
    pub(crate) fn is_isolated_from(&self, other: &Self) -> bool {
        !self.documents.is_same_instance(&other.documents)
            && !self
                .prepared_documents
                .is_same_instance(&other.prepared_documents)
            && !Arc::ptr_eq(&self.codec, &other.codec)
    }
}

pub fn prepare_open_input(
    service: &DocumentOpenService,
    source: OpenDocumentSource,
) -> Result<PreparedOpenDocument, AppError> {
    let source_path = PathBuf::from(&source.path);
    let plan = service.codec().plan_open(&source)?;
    let reservation = service.prepared_documents().reserve_for_parse_bytes(
        plan.estimated_parse_bytes(),
        active_document_resource_bytes(service)?,
    )?;
    let editor_state = plan.decode(source)?;

    prepare_editor_state(service, editor_state, Some(source_path), reservation)
}

pub fn prepare_new_file(service: &DocumentOpenService) -> Result<PreparedOpenDocument, AppError> {
    let file_data = blank_file_data();
    validate_file_data(&file_data)?;
    let reservation = service
        .prepared_documents()
        .reserve_for_file_data(&file_data, active_document_resource_bytes(service)?)?;
    prepare_editor_state(
        service,
        EditorState::with_workbook(file_data, None),
        None,
        reservation,
    )
}

pub fn abort_prepared_document(service: &DocumentOpenService, token: &str) -> Result<(), AppError> {
    service.prepared_documents().abort(token)
}

fn blank_file_data() -> FileData {
    FileData {
        path: String::new(),
        file_name: format!("untitled.{}", default_spreadsheet_extension()),
        sheets: vec![SheetData {
            name: "Sheet1".to_string(),
            rows: vec![vec![crate::types::CellValue::Null; 5]; 5],
            ..Default::default()
        }],
    }
}

fn prepare_editor_state(
    service: &DocumentOpenService,
    editor_state: EditorState,
    source_path: Option<PathBuf>,
    reservation: prepared_document_repository::PrepareReservation,
) -> Result<PreparedOpenDocument, AppError> {
    let token = service.prepared_documents().replace(
        editor_state,
        source_path,
        reservation,
        active_document_resource_bytes(service)?,
    )?;
    Ok(PreparedOpenDocument { token })
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
    use crate::types::CellValue;

    struct TestCodec;
    struct TestDecodePlan;

    impl crate::application::document_codec_port::DocumentDecodePlan for TestDecodePlan {
        fn estimated_parse_bytes(&self) -> usize {
            1024
        }

        fn decode(self: Box<Self>, source: OpenDocumentSource) -> Result<EditorState, AppError> {
            let text = String::from_utf8(source.bytes)
                .map_err(|error| AppError::ReadError(error.to_string()))?;
            Ok(EditorState::with_workbook(
                FileData {
                    path: source.path,
                    file_name: source
                        .file_name
                        .unwrap_or_else(|| "unknown.csv".to_string()),
                    sheets: vec![SheetData {
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

        fn decode_saved(
            &self,
            _extension: &str,
            _bytes: Vec<u8>,
            _path: String,
            _file_name: String,
        ) -> Result<crate::document::document_model::SpreadsheetDocument, AppError> {
            unreachable!("open-service tests do not reparse saved files")
        }
    }

    fn service() -> DocumentOpenService {
        DocumentOpenService::new(
            ActiveDocumentRepository::default(),
            PreparedDocumentRepository::default(),
            Arc::new(TestCodec),
        )
    }

    #[test]
    fn open_input_is_decoded_through_the_injected_codec() {
        let service = service();
        let prepared = prepare_open_input(
            &service,
            OpenDocumentSource {
                path: "/tmp/imported".to_string(),
                bytes: b"decoded through port".to_vec(),
                file_name: Some("imported.csv".to_string()),
            },
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
    }

    #[test]
    fn new_file_uses_the_backend_owned_blank_template() {
        let service = service();
        let prepared = prepare_new_file(&service).expect("init file");
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
}
