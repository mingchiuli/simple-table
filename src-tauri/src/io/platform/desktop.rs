use crate::error::AppError;
use crate::types::FileData;
use std::fs;

pub fn read_file(path: &str) -> Result<FileData, AppError> {
    let bytes = fs::read(path).map_err(|e| AppError::ReadError(e.to_string()))?;
    crate::io::document::open_from_bytes(path.to_string(), bytes, None)
}

pub fn save_file(path: &str, file_data: &FileData) -> Result<(), AppError> {
    let (_, bytes) = crate::io::codec::writer::generate_file_bytes(file_data)?;
    fs::write(path, bytes).map_err(|e| AppError::WriteError(e.to_string()))?;
    Ok(())
}
