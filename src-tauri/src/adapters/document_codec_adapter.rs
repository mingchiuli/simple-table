use crate::application::document_codec_port::{
    DocumentCodecPort, DocumentDecodePlan, OpenDocumentSource,
};
use crate::application::document_encode_port::DocumentEncodePort;
use crate::document::backing::document_body::SpreadsheetDocumentBody;
use crate::document::document_model::SpreadsheetDocument;
use crate::document::document_save::{DocumentSaveEncoding, SpreadsheetDocumentSaveSnapshot};
use crate::document_format::{file_name_from_path_like, open_extension_from_path_name_or_bytes};
use crate::error::AppError;
use crate::io::codec::reader::{
    InputFilePreflight, preflight_input_file, read_file_with_workbook_from_bytes,
    read_file_with_workbook_from_preflight,
};
use crate::io::codec::writer;
use crate::state::editor_state::EditorState;

#[derive(Default)]
pub(crate) struct DocumentCodecAdapter;

struct IoDocumentDecodePlan {
    preflight: InputFilePreflight,
}

impl DocumentDecodePlan for IoDocumentDecodePlan {
    fn estimated_parse_bytes(&self) -> usize {
        self.preflight.estimated_parse_bytes()
    }

    fn decode(self: Box<Self>, source: OpenDocumentSource) -> Result<EditorState, AppError> {
        let resolved_file_name = source
            .file_name
            .unwrap_or_else(|| file_name_from_path_like(&source.path, "unknown"));
        let result = read_file_with_workbook_from_preflight(
            self.preflight,
            source.bytes,
            source.path,
            resolved_file_name,
        )?;
        let body = SpreadsheetDocumentBody::from_projection(&result.file_data, result.workbook);
        Ok(EditorState::from_document(
            SpreadsheetDocument::from_backing(result.file_data, body),
        ))
    }
}

impl DocumentEncodePort for DocumentCodecAdapter {
    fn encode(
        &self,
        snapshot: &SpreadsheetDocumentSaveSnapshot,
        target_path_or_name: &str,
    ) -> Result<(String, Vec<u8>), AppError> {
        match snapshot.encoding()? {
            DocumentSaveEncoding::NativeWorkbook(workbook) => {
                writer::generate_excel_bytes_from_workbook_for_target(workbook, target_path_or_name)
            }
            DocumentSaveEncoding::Projection(projection) => {
                writer::generate_file_bytes_for_target(projection, target_path_or_name)
            }
        }
    }
}

impl DocumentCodecPort for DocumentCodecAdapter {
    fn plan_open(
        &self,
        source: &OpenDocumentSource,
    ) -> Result<Box<dyn DocumentDecodePlan>, AppError> {
        let extension = open_extension_from_path_name_or_bytes(
            &source.path,
            source.file_name.as_deref(),
            &source.bytes,
        );
        Ok(Box::new(IoDocumentDecodePlan {
            preflight: preflight_input_file(&extension, &source.bytes)?,
        }))
    }

    fn decode_saved(
        &self,
        extension: &str,
        bytes: Vec<u8>,
        path: String,
        file_name: String,
    ) -> Result<SpreadsheetDocument, AppError> {
        let result = read_file_with_workbook_from_bytes(extension, bytes, path, file_name)?;
        let body = SpreadsheetDocumentBody::from_projection(&result.file_data, result.workbook);
        Ok(SpreadsheetDocument::from_backing(result.file_data, body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_data::{DocumentData, DocumentSheet};
    use crate::types::CellValue;

    #[test]
    fn extensionless_csv_source_is_decoded_through_the_port() {
        let adapter = DocumentCodecAdapter;
        let source = OpenDocumentSource {
            path: "/tmp/imported".to_string(),
            bytes: b"name,score\nalice,42".to_vec(),
            file_name: Some("imported".to_string()),
        };
        let plan = adapter.plan_open(&source).expect("plan CSV");
        let state = plan.decode(source).expect("decode CSV");
        let rows = &state.file_data().sheets[0].rows;

        assert_eq!(rows[0][0], CellValue::String("name".to_string()));
        assert_eq!(rows[1][1], CellValue::Number(42.into()));
    }

    #[test]
    fn projection_snapshot_is_encoded_through_the_port() {
        let document = SpreadsheetDocument::new(DocumentData {
            path: String::new(),
            file_name: "source.csv".to_string(),
            sheets: vec![DocumentSheet {
                rows: vec![vec![CellValue::String("encoded".to_string())]],
                ..Default::default()
            }],
        });
        let snapshot = document
            .save_snapshot_for_target("export.csv")
            .expect("save snapshot");

        let (output_name, bytes) = DocumentCodecAdapter
            .encode(&snapshot, "export.csv")
            .expect("encode snapshot");

        assert_eq!(output_name, "export.csv");
        assert_eq!(String::from_utf8(bytes).expect("UTF-8 CSV"), "encoded\n");
    }
}
