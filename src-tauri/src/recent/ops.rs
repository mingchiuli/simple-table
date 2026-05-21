use tauri::AppHandle;

use crate::error::AppError;
use crate::recent::types::StorageType;

use super::store::RecentStore;
use super::thumbnail::generate_thumbnail_from_bytes;
use super::types::RecentFile;

pub fn do_get_recent_files(app: &AppHandle) -> Vec<RecentFile> {
    RecentStore::get_all(app)
}

pub fn do_add_recent_file_with_thumbnail(
    app: &AppHandle,
    path: String,
    file_name: String,
    file_size: i64,
    bytes: Vec<u8>,
    extension: String,
    storage_type: Option<String>,
    original_path: Option<String>,
) -> Result<RecentFile, AppError> {
    let mut recent_file = RecentFile::new(path, file_name, file_size);

    if let Some(thumbnail) = generate_thumbnail_from_bytes(&bytes, &extension) {
        recent_file.thumbnail = Some(thumbnail);
    }

    // Convert string to StorageType
    if let Some(st) = storage_type {
        recent_file.storage_type = match st.as_str() {
            "mobileSandboxPath" => StorageType::MobileSandboxPath,
            "desktopPath" => StorageType::DesktopPath,
            _ => StorageType::default(),
        };
    }

    if let Some(op) = original_path {
        recent_file.original_path = Some(op);
    }

    RecentStore::add(app, recent_file)
}

pub fn do_remove_recent_file(app: &AppHandle, id: String) -> Result<(), AppError> {
    RecentStore::remove(app, &id)
}

pub fn do_check_file_exists(path: String) -> bool {
    RecentStore::exists(&path)
}

pub fn do_update_recent_file_path(
    app: &AppHandle,
    id: String,
    new_path: String,
) -> Result<(), AppError> {
    RecentStore::update_path(app, &id, &new_path)
}
