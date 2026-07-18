use crate::application::document_codec_port::{
    DocumentCodecPort, DocumentDecodePlan, OpenDocumentSource,
};
use crate::document::document_model::SpreadsheetDocument;
use crate::document_format::{file_name_from_path_like, open_extension_from_path_name_or_bytes};
use crate::error::AppError;
use crate::io::codec::reader::{
    InputFilePreflight, preflight_input_file, read_file_with_workbook_from_bytes,
    read_file_with_workbook_from_preflight,
};
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
        Ok(EditorState::with_workbook(
            result.file_data,
            result.workbook,
        ))
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
        Ok(SpreadsheetDocument::new(result.file_data, result.workbook))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
