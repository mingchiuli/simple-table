use crate::document::document_save::SpreadsheetDocumentSaveSnapshot;
use crate::error::AppError;

pub(crate) trait DocumentEncodePort: Send + Sync {
    fn encode(
        &self,
        snapshot: &SpreadsheetDocumentSaveSnapshot,
        target_path_or_name: &str,
    ) -> Result<(String, Vec<u8>), AppError>;
}
