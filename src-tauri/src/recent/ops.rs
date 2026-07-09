use tauri::AppHandle;

use crate::error::AppError;
use crate::io::document;
use crate::recent::types::StorageType;
use serde::Deserialize;

use super::store::RecentStore;
use super::thumbnail::generate_thumbnail_from_file_data;
use super::types::RecentFile;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddRecentFileRequest {
    pub path: String,
    pub file_name: String,
    pub file_size: i64,
    pub storage_type: Option<String>,
    pub original_path: Option<String>,
    pub document_id: Option<u64>,
    pub base_revision: Option<u64>,
}

pub fn do_get_recent_files(app: &AppHandle) -> Vec<RecentFile> {
    RecentStore::get_all(app)
}

pub fn do_add_recent_file_with_thumbnail(
    app: &AppHandle,
    request: AddRecentFileRequest,
) -> Result<RecentFile, AppError> {
    let AddRecentFileRequest {
        path,
        file_name,
        file_size,
        storage_type,
        original_path,
        document_id,
        base_revision,
    } = request;
    let mut recent_file = RecentFile::new(path, file_name, file_size);

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

    match (document_id, base_revision) {
        (Some(document_id), Some(base_revision)) => {
            if let Ok(file_data) =
                document::current_file_data_for_command(document_id, base_revision)
                && let Some(thumbnail) = generate_thumbnail_from_file_data(&file_data)
            {
                recent_file.thumbnail = Some(thumbnail);
            }
        }
        (None, None) => {}
        _ => {
            return Err(AppError::DocumentStateInvalid(
                "recent file thumbnail request must include both documentId and baseRevision"
                    .to_string(),
            ));
        }
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
