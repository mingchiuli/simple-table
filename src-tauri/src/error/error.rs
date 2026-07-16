use serde::Serialize;
use serde::ser::SerializeStruct;
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum AppError {
    // 文件操作
    #[error("Failed to read file: {0}")]
    ReadError(String),
    #[error("Failed to write file: {0}")]
    WriteError(String),
    #[error("File not found: {0}")]
    FileNotFound(String),

    // 格式操作
    #[error("Unsupported file format")]
    UnsupportedFormat,
    #[error("Document exceeds the supported resource limits: {0}")]
    ResourceLimitExceeded(String),
    #[error("Sheet region response is {estimated_bytes} bytes, maximum is {maximum_bytes} bytes")]
    RegionResponseTooLarge {
        estimated_bytes: usize,
        maximum_bytes: usize,
    },
    #[error("Another prepared document is still active")]
    PreparedDocumentConflict,
    #[error("Update check failed: {0}")]
    UpdateError(String),

    // 状态操作
    #[error("No file loaded")]
    NoFileLoaded,
    #[error("Invalid sheet index: {0}")]
    InvalidSheetIndex(usize),
    #[error("Invalid cell position: row {row}, col {col}")]
    InvalidCellPosition { row: usize, col: usize },
    #[error("Row not found: {0}")]
    RowNotFound(usize),
    #[error("Nothing to undo")]
    NothingToUndo,
    #[error("Nothing to redo")]
    NothingToRedo,
    #[error("Cannot delete the last sheet")]
    CannotDeleteLastSheet,
    #[error("Workbook patch failed: {0}")]
    WorkbookPatchFailed(String),
    #[error(
        "Editor transaction failed and rollback also failed. Operation error: {operation_error}; rollback error: {rollback_error}"
    )]
    TransactionRollbackFailed {
        operation_error: String,
        rollback_error: String,
    },
    #[error("Document state is unavailable after a failed transaction: {0}")]
    DocumentStateInvalid(String),
    #[error(
        "Structure editing is disabled for this workbook because it contains unsupported Excel features: {0}"
    )]
    UnsupportedWorkbookStructure(String),

    // 内部错误
    #[error("Internal error: {0}")]
    Internal(String),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("AppError", 2)?;
        state.serialize_field("code", self.code())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ReadError(_) => "read_error",
            Self::WriteError(_) => "write_error",
            Self::FileNotFound(_) => "file_not_found",
            Self::UnsupportedFormat => "unsupported_format",
            Self::ResourceLimitExceeded(_) => "resource_limit_exceeded",
            Self::RegionResponseTooLarge { .. } => "region_response_too_large",
            Self::PreparedDocumentConflict => "prepared_document_conflict",
            Self::UpdateError(_) => "update_error",
            Self::NoFileLoaded => "no_file_loaded",
            Self::InvalidSheetIndex(_) => "invalid_sheet_index",
            Self::InvalidCellPosition { .. } => "invalid_cell_position",
            Self::RowNotFound(_) => "row_not_found",
            Self::NothingToUndo => "nothing_to_undo",
            Self::NothingToRedo => "nothing_to_redo",
            Self::CannotDeleteLastSheet => "cannot_delete_last_sheet",
            Self::WorkbookPatchFailed(_) => "workbook_patch_failed",
            Self::TransactionRollbackFailed { .. } => "transaction_rollback_failed",
            Self::DocumentStateInvalid(_) => "document_state_invalid",
            Self::UnsupportedWorkbookStructure(_) => "unsupported_workbook_structure",
            Self::Internal(_) => "internal",
        }
    }

    pub fn poisoned_lock(name: &'static str) -> Self {
        Self::Internal(format!("{name} lock poisoned"))
    }
}

#[cfg(test)]
mod tests {
    use super::AppError;
    use serde_json::json;

    #[test]
    fn serializes_stable_error_code_and_human_message() {
        assert_eq!(
            serde_json::to_value(AppError::FileNotFound("/tmp/missing.xlsx".to_string()))
                .expect("serialize error"),
            json!({
                "code": "file_not_found",
                "message": "File not found: /tmp/missing.xlsx",
            })
        );
    }

    #[test]
    fn serializes_region_response_limit_with_a_distinct_code() {
        assert_eq!(
            serde_json::to_value(AppError::RegionResponseTooLarge {
                estimated_bytes: 20,
                maximum_bytes: 10,
            })
            .expect("serialize error"),
            json!({
                "code": "region_response_too_large",
                "message": "Sheet region response is 20 bytes, maximum is 10 bytes",
            })
        );
    }

    #[test]
    fn serializes_update_failures_with_a_distinct_code() {
        assert_eq!(
            serde_json::to_value(AppError::UpdateError("request timed out".to_string()))
                .expect("serialize error"),
            json!({
                "code": "update_error",
                "message": "Update check failed: request timed out",
            })
        );
    }
}
