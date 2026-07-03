use crate::error::AppError;
use crate::io::document;
use crate::types::OpenDocumentResponse;
use std::fs;

pub fn read_file(path: &str) -> Result<OpenDocumentResponse, AppError> {
    let bytes = fs::read(path).map_err(|e| AppError::ReadError(e.to_string()))?;
    document::open_from_bytes(path.to_string(), bytes, None)
}

pub fn save_file(path: &str) -> Result<(), AppError> {
    let (_, bytes) = document::generate_current_file_bytes_for_target(path)?;
    fs::write(path, bytes).map_err(|e| AppError::WriteError(e.to_string()))?;
    Ok(())
}
