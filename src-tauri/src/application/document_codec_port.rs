use crate::document::document_model::SpreadsheetDocument;
use crate::error::AppError;
use crate::state::editor_state::EditorState;

pub struct OpenDocumentSource {
    pub path: String,
    pub bytes: Vec<u8>,
    pub file_name: Option<String>,
}

pub(crate) trait DocumentDecodePlan: Send {
    fn estimated_parse_bytes(&self) -> usize;
    fn decode(self: Box<Self>, source: OpenDocumentSource) -> Result<EditorState, AppError>;
}

pub(crate) trait DocumentCodecPort: Send + Sync {
    fn plan_open(
        &self,
        source: &OpenDocumentSource,
    ) -> Result<Box<dyn DocumentDecodePlan>, AppError>;

    fn decode_saved(
        &self,
        extension: &str,
        bytes: Vec<u8>,
        path: String,
        file_name: String,
    ) -> Result<SpreadsheetDocument, AppError>;
}
