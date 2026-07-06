use crate::error::AppError;
use crate::io::document;
use crate::types::{OpenDocumentResponse, SavedDocumentResponse};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};
use tauri_plugin_fs::FilePath;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedFileInfo {
    pub path: String,
    pub original_path: String,
    pub file_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickFileResult {
    #[serde(flatten)]
    pub document: OpenDocumentResponse,
    pub info: PickedFileInfo,
}

pub(super) fn extension_from_name(file_name: &str) -> String {
    Path::new(file_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| !ext.is_empty())
        .unwrap_or("xlsx")
        .to_string()
}

pub(super) fn mobile_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| AppError::ReadError(format!("Failed to get app local data dir: {}", e)))?
        .join("files");
    fs::create_dir_all(&dir)
        .map_err(|e| AppError::WriteError(format!("Failed to create app file dir: {}", e)))?;
    Ok(dir)
}

pub(super) fn unique_import_path(app: &AppHandle, file_name: &str) -> Result<PathBuf, AppError> {
    Ok(mobile_dir(app)?.join(format!(
        "{}.{}",
        uuid::Uuid::new_v4(),
        extension_from_name(file_name)
    )))
}

pub(super) fn write_with_official_fs(
    app: &AppHandle,
    path: FilePath,
    bytes: &[u8],
) -> Result<(), AppError> {
    use tauri_plugin_fs::{FsExt, OpenOptions};

    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    let mut file = app
        .fs()
        .open(path, options)
        .map_err(|e| AppError::WriteError(format!("Failed to open file for writing: {}", e)))?;
    file.write_all(bytes)
        .map_err(|e| AppError::WriteError(format!("Failed to write file: {}", e)))
}

pub(super) fn write_path_with_official_fs(
    app: &AppHandle,
    path: PathBuf,
    bytes: &[u8],
) -> Result<(), AppError> {
    write_with_official_fs(app, FilePath::from(path), bytes)
}

pub fn read_file(app: &AppHandle, path: &str) -> Result<OpenDocumentResponse, AppError> {
    use tauri_plugin_fs::FsExt;

    let file_name = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string();
    let bytes = app
        .fs()
        .read(FilePath::from(PathBuf::from(path)))
        .map_err(|e| AppError::ReadError(format!("Failed to read file: {}", e)))?;

    document::open_from_bytes(path.to_string(), bytes, Some(file_name))
}

pub fn save_file(_app: &AppHandle, path: &str) -> Result<SavedDocumentResponse, AppError> {
    let prepared = document::prepare_current_file_save(path)?;
    let target = PathBuf::from(path);
    let temp_path = temporary_path_for(&target);
    write_local_temp_file(&temp_path, &prepared.bytes)?;

    let result = document::commit_current_file_save(path.to_string(), prepared, || {
        replace_local_file_with_temp(&temp_path, &target)
    });
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

pub fn create_file(app: &AppHandle, file_name: &str) -> Result<PickedFileInfo, AppError> {
    let path = mobile_dir(app)?.join(format!(
        "{}.{}",
        uuid::Uuid::new_v4(),
        extension_from_name(file_name)
    ));
    write_path_with_official_fs(app, path.clone(), &[])?;

    Ok(PickedFileInfo {
        path: path.to_string_lossy().to_string(),
        original_path: String::new(),
        file_name: file_name.to_string(),
    })
}

pub fn export_file(
    app: &AppHandle,
    source_path: &str,
    default_name: &str,
) -> Result<Option<String>, AppError> {
    use tauri_plugin_dialog::{DialogExt, PickerMode};
    use tauri_plugin_fs::FsExt;

    let dest = match app
        .dialog()
        .file()
        .add_filter("Spreadsheet", &["xlsx", "csv", "*"])
        .set_picker_mode(PickerMode::Document)
        .set_file_name(default_name)
        .blocking_save_file()
    {
        Some(path) => path,
        None => return Ok(None),
    };

    let bytes = app
        .fs()
        .read(FilePath::from(PathBuf::from(source_path)))
        .map_err(|e| AppError::ReadError(format!("Failed to read export source: {}", e)))?;

    write_with_official_fs(app, dest.clone(), &bytes)
        .map_err(|e| AppError::WriteError(format!("Failed to export file: {}", e)))?;

    Ok(Some(dest.to_string()))
}

fn temporary_path_for(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("simple-table.xlsx");
    parent.join(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()))
}

fn write_local_temp_file(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let mut file = fs::File::create(path).map_err(|e| AppError::WriteError(e.to_string()))?;
    file.write_all(bytes)
        .map_err(|e| AppError::WriteError(e.to_string()))?;
    file.sync_all()
        .map_err(|e| AppError::WriteError(e.to_string()))
}

fn replace_local_file_with_temp(temp_path: &Path, target: &Path) -> Result<(), AppError> {
    match fs::rename(temp_path, target) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            let _ = fs::remove_file(temp_path);
            Err(AppError::WriteError(rename_error.to_string()))
        }
    }
}
