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
    #[error("Sheet region response is {wire_bytes} bytes, maximum is {maximum_bytes} bytes")]
    RegionResponseTooLarge {
        wire_bytes: usize,
        maximum_bytes: usize,
    },
    #[error("Another prepared document is still active")]
    PreparedDocumentConflict,
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
    use crate::protocol::KNOWN_ERROR_CODES;
    use serde_json::json;

    fn every_variant() -> [AppError; 19] {
        [
            AppError::ReadError(String::new()),
            AppError::WriteError(String::new()),
            AppError::FileNotFound(String::new()),
            AppError::UnsupportedFormat,
            AppError::ResourceLimitExceeded(String::new()),
            AppError::RegionResponseTooLarge {
                wire_bytes: 0,
                maximum_bytes: 0,
            },
            AppError::PreparedDocumentConflict,
            AppError::NoFileLoaded,
            AppError::InvalidSheetIndex(0),
            AppError::InvalidCellPosition { row: 0, col: 0 },
            AppError::RowNotFound(0),
            AppError::NothingToUndo,
            AppError::NothingToRedo,
            AppError::CannotDeleteLastSheet,
            AppError::WorkbookPatchFailed(String::new()),
            AppError::TransactionRollbackFailed {
                operation_error: String::new(),
                rollback_error: String::new(),
            },
            AppError::DocumentStateInvalid(String::new()),
            AppError::UnsupportedWorkbookStructure(String::new()),
            AppError::Internal(String::new()),
        ]
    }

    #[test]
    fn every_error_code_is_registered_in_the_protocol_contract() {
        for error in every_variant() {
            assert!(
                KNOWN_ERROR_CODES.contains(&error.code()),
                "{} must be registered in KNOWN_ERROR_CODES",
                error.code()
            );
        }
    }

    #[test]
    fn the_registered_variant_list_covers_every_app_error() {
        // Exhaustive matches break compilation when a variant is added, which
        // forces the coverage test above to be updated as well.
        for error in every_variant() {
            match error {
                AppError::ReadError(_)
                | AppError::WriteError(_)
                | AppError::FileNotFound(_)
                | AppError::UnsupportedFormat
                | AppError::ResourceLimitExceeded(_)
                | AppError::RegionResponseTooLarge { .. }
                | AppError::PreparedDocumentConflict
                | AppError::NoFileLoaded
                | AppError::InvalidSheetIndex(_)
                | AppError::InvalidCellPosition { .. }
                | AppError::RowNotFound(_)
                | AppError::NothingToUndo
                | AppError::NothingToRedo
                | AppError::CannotDeleteLastSheet
                | AppError::WorkbookPatchFailed(_)
                | AppError::TransactionRollbackFailed { .. }
                | AppError::DocumentStateInvalid(_)
                | AppError::UnsupportedWorkbookStructure(_)
                | AppError::Internal(_) => {}
            }
        }
    }

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
                wire_bytes: 20,
                maximum_bytes: 10,
            })
            .expect("serialize error"),
            json!({
                "code": "region_response_too_large",
                "message": "Sheet region response is 20 bytes, maximum is 10 bytes",
            })
        );
    }
}
