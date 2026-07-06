use crate::error::AppError;
use crate::io::document;
use crate::types::{OpenDocumentResponse, SavedDocumentResponse};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn read_file(path: &str) -> Result<OpenDocumentResponse, AppError> {
    let bytes = fs::read(path).map_err(|e| AppError::ReadError(e.to_string()))?;
    document::open_from_bytes(path.to_string(), bytes, None)
}

pub fn save_file(path: &str) -> Result<SavedDocumentResponse, AppError> {
    let prepared = document::prepare_current_file_save(path)?;
    write_file_atomically(Path::new(path), &prepared.bytes)?;
    document::complete_current_file_save(path.to_string(), prepared)
}

pub fn export_file(path: &str) -> Result<(), AppError> {
    let (_, bytes) = document::generate_current_file_bytes_for_target(path)?;
    write_file_atomically(Path::new(path), &bytes)
}

fn write_file_atomically(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("simple-table.xlsx");
    let temp_path = parent.join(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()));

    write_temp_file(&temp_path, bytes)?;
    match fs::rename(&temp_path, path) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            let _ = fs::remove_file(&temp_path);
            Err(AppError::WriteError(rename_error.to_string()))
        }
    }
}

fn write_temp_file(path: &PathBuf, bytes: &[u8]) -> Result<(), AppError> {
    let mut file = fs::File::create(path).map_err(|e| AppError::WriteError(e.to_string()))?;
    file.write_all(bytes)
        .map_err(|e| AppError::WriteError(e.to_string()))?;
    file.sync_all()
        .map_err(|e| AppError::WriteError(e.to_string()))
}
