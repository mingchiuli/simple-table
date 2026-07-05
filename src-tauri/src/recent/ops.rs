use tauri::AppHandle;

use crate::error::AppError;
use crate::recent::types::StorageType;
use serde::Deserialize;

use super::store::RecentStore;
use super::thumbnail::generate_thumbnail_from_bytes;
use super::types::RecentFile;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddRecentFileRequest {
    pub path: String,
    pub file_name: String,
    pub file_size: i64,
    pub bytes: Vec<u8>,
    pub storage_type: Option<String>,
    pub original_path: Option<String>,
}

pub fn do_get_recent_files(app: &AppHandle) -> Vec<RecentFile> {
    RecentStore::get_all(app)
}

pub fn do_add_recent_file_with_thumbnail(
    app: &AppHandle,
    request: AddRecentFileRequest,
) -> Result<RecentFile, AppError> {
    let mut recent_file = RecentFile::new(request.path, request.file_name, request.file_size);

    if let Some(thumbnail) = generate_thumbnail_from_bytes(&request.bytes, "xlsx") {
        recent_file.thumbnail = Some(thumbnail);
    }

    // Convert string to StorageType
    if let Some(st) = request.storage_type {
        recent_file.storage_type = match st.as_str() {
            "mobileSandboxPath" => StorageType::MobileSandboxPath,
            "desktopPath" => StorageType::DesktopPath,
            _ => StorageType::default(),
        };
    }

    if let Some(op) = request.original_path {
        recent_file.original_path = Some(op);
    }

    RecentStore::add(app, recent_file)
}

pub fn do_remove_recent_file(app: &AppHandle, id: &str) -> Result<(), AppError> {
    RecentStore::remove(app, id)
}

pub fn do_check_file_exists(path: &str) -> bool {
    RecentStore::exists(path)
}

pub fn do_update_recent_file_path(
    app: &AppHandle,
    id: &str,
    new_path: &str,
) -> Result<(), AppError> {
    RecentStore::update_path(app, id, new_path)
}
