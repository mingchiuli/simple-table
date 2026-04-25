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
    #[error("File permission denied: {0}")]
    FilePermissionDenied(String),

    // 格式操作
    #[error("Unsupported file format")]
    UnsupportedFormat,
    #[error("Invalid file format: {0}")]
    InvalidFormat(String),

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

    // 内部错误
    #[error("Lock poisoned: {0}")]
    LockPoisoned(String),
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
