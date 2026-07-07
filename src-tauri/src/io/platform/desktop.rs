use crate::error::AppError;
use crate::io::atomic_file::{
    cleanup_temp_file, replace_temp_file, write_file_atomically, write_temp_file_for_target,
};
use crate::io::document;
use crate::types::{OpenDocumentResponse, SavedDocumentResponse};
use std::fs;
use std::path::Path;

pub fn read_file(path: &str) -> Result<OpenDocumentResponse, AppError> {
    let bytes = fs::read(path).map_err(|e| AppError::ReadError(e.to_string()))?;
    document::open_from_bytes(path.to_string(), bytes, None)
}

pub fn save_file(path: &str) -> Result<SavedDocumentResponse, AppError> {
    let prepared = document::prepare_current_file_save(path)?;
    let target = Path::new(path);
    let temp_path = write_temp_file_for_target(target, &prepared.bytes)?;

    let result = document::commit_current_file_save(path.to_string(), prepared, || {
        replace_temp_file(&temp_path, target)
    });
    if result.is_err() {
        cleanup_temp_file(&temp_path);
    }
    result
}

pub fn export_file(path: &str) -> Result<(), AppError> {
    let (_, bytes) = document::generate_current_file_bytes_for_target(path)?;
    write_file_atomically(Path::new(path), &bytes)
}
