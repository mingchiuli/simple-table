use crate::error::AppError;
use crate::io::document_body::SpreadsheetDocumentBodySnapshot;
use crate::types::FileData;

pub struct SpreadsheetDocumentSaveSnapshot {
    projection: SaveProjectionSnapshot,
    body: SpreadsheetDocumentBodySnapshot,
    transaction_failure: Option<String>,
}

enum SaveProjectionSnapshot {
    ValidatedNativeWorkbook,
    Projection(FileData),
}

impl SpreadsheetDocumentSaveSnapshot {
    pub(crate) fn validated_native_workbook(
        body: SpreadsheetDocumentBodySnapshot,
        transaction_failure: Option<String>,
    ) -> Self {
        Self {
            projection: SaveProjectionSnapshot::ValidatedNativeWorkbook,
            body,
            transaction_failure,
        }
    }

    pub(crate) fn projection(
        projection: FileData,
        body: SpreadsheetDocumentBodySnapshot,
        transaction_failure: Option<String>,
    ) -> Self {
        Self {
            projection: SaveProjectionSnapshot::Projection(projection),
            body,
            transaction_failure,
        }
    }

    pub fn is_excel_backed(&self) -> bool {
        self.body.is_excel_backed()
    }

    pub fn generate_file_bytes_for_target(
        &self,
        target_path_or_name: &str,
    ) -> Result<(String, Vec<u8>), AppError> {
        if let Some(reason) = &self.transaction_failure {
            return Err(AppError::DocumentStateInvalid(reason.clone()));
        }
        match &self.projection {
            SaveProjectionSnapshot::Projection(projection) => {
                self.body
                    .validate_persisted_projection_consistency(projection)?;
                self.body
                    .generate_file_bytes_for_target(projection, target_path_or_name)
            }
            SaveProjectionSnapshot::ValidatedNativeWorkbook => self
                .body
                .generate_file_bytes_without_projection_for_target(target_path_or_name),
        }
    }
}
