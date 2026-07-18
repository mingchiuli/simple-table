use crate::document::backing::document_body::SpreadsheetDocumentBodySnapshot;
use crate::document_data::DocumentData;
use crate::error::AppError;
use umya_spreadsheet::Workbook;

pub(crate) enum DocumentSaveEncoding<'a> {
    NativeWorkbook(&'a Workbook),
    Projection(&'a DocumentData),
}

pub struct SpreadsheetDocumentSaveSnapshot {
    projection: SaveProjectionSnapshot,
    body: SpreadsheetDocumentBodySnapshot,
    transaction_failure: Option<String>,
}

enum SaveProjectionSnapshot {
    ValidatedNativeWorkbook,
    Projection(DocumentData),
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
        projection: DocumentData,
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

    pub(crate) fn encoding(&self) -> Result<DocumentSaveEncoding<'_>, AppError> {
        if let Some(reason) = &self.transaction_failure {
            return Err(AppError::DocumentStateInvalid(reason.clone()));
        }
        match &self.projection {
            SaveProjectionSnapshot::Projection(projection) => {
                self.body
                    .validate_persisted_projection_consistency(projection)?;
                Ok(DocumentSaveEncoding::Projection(projection))
            }
            SaveProjectionSnapshot::ValidatedNativeWorkbook => self
                .body
                .native_workbook()
                .map(DocumentSaveEncoding::NativeWorkbook)
                .ok_or_else(|| {
                    AppError::Internal(
                        "validated native save snapshot has no workbook backing".to_string(),
                    )
                }),
        }
    }
}
