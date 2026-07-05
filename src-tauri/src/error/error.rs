use serde::Serialize;
use thiserror::Error;

#[derive(Error, Debug)]
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
        serializer.serialize_str(&self.to_string())
    }
}

impl AppError {
    pub fn poisoned_lock(name: &'static str) -> Self {
        Self::Internal(format!("{name} lock poisoned"))
    }
}
