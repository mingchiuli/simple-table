use crate::error::AppError;
use crate::io::{codec::writer, document};
use crate::types::FileData;
use std::fs;

pub fn read_file(path: &str) -> Result<FileData, AppError> {
    let bytes = fs::read(path).map_err(|e| AppError::ReadError(e.to_string()))?;
    document::open_from_bytes(path.to_string(), bytes, None)
}

pub fn save_file(path: &str, file_data: &FileData) -> Result<(), AppError> {
    let (_, bytes) = writer::generate_file_bytes_for_target(file_data, path)?;
    fs::write(path, bytes).map_err(|e| AppError::WriteError(e.to_string()))?;
    Ok(())
}
