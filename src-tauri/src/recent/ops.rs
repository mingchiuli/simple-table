use tauri::AppHandle;

use crate::error::AppError;
use crate::io::document;

use super::store::RecentStore;
use super::thumbnail::generate_thumbnail_from_file_data;
use super::types::{AddRecentFileRequest, RecentFile, StorageType};

pub fn do_get_recent_files(app: &AppHandle) -> Vec<RecentFile> {
    RecentStore::get_all(app)
}

pub fn do_add_recent_file_with_thumbnail(
    app: &AppHandle,
    request: AddRecentFileRequest,
) -> Result<RecentFile, AppError> {
    let AddRecentFileRequest {
        original_path,
        document_id,
        base_revision,
    } = request;
    let file_data = document::current_file_data_for_command(document_id, base_revision)?;

    if file_data.path.is_empty() {
        return Err(AppError::DocumentStateInvalid(
            "cannot add an unsaved document to recent files".to_string(),
        ));
    }

    let file_size = recent_file_size(&file_data.path);
    let mut recent_file = RecentFile::new(
        file_data.path.clone(),
        file_data.file_name.clone(),
        file_size,
    );
    recent_file.storage_type = current_platform_storage_type();

    if let Some(op) = original_path {
        recent_file.original_path = Some(op);
    }

    if let Some(thumbnail) = generate_thumbnail_from_file_data(&file_data) {
        recent_file.thumbnail = Some(thumbnail);
    }

    RecentStore::add(app, recent_file)
}

pub fn do_remove_recent_file(app: &AppHandle, id: &str) -> Result<(), AppError> {
    RecentStore::remove(app, id)
}

fn recent_file_size(path: &str) -> i64 {
    std::fs::metadata(path)
        .map(|metadata| metadata.len() as i64)
        .unwrap_or_default()
}

fn current_platform_storage_type() -> StorageType {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        StorageType::MobileSandboxPath
    } else {
        StorageType::DesktopPath
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recent::types::StorageType;
    use serde_json::json;

    #[test]
    fn add_recent_request_uses_generated_storage_type_contract() {
        let request: AddRecentFileRequest = serde_json::from_value(json!({
            "documentId": 7,
            "baseRevision": 3,
            "originalPath": "/original/book.xlsx"
        }))
        .expect("recent request");

        assert_eq!(request.document_id, 7);
        assert_eq!(request.base_revision, 3);
        assert_eq!(
            request.original_path.as_deref(),
            Some("/original/book.xlsx")
        );
    }

    #[test]
    fn add_recent_request_requires_document_context() {
        let error = serde_json::from_value::<AddRecentFileRequest>(json!({
            "originalPath": "/original/book.xlsx"
        }))
        .expect_err("document context should be required");

        assert!(error.to_string().contains("missing field"));
    }

    #[test]
    fn platform_storage_type_matches_target_family() {
        let storage_type = current_platform_storage_type();

        if cfg!(any(target_os = "android", target_os = "ios")) {
            assert_eq!(storage_type, StorageType::MobileSandboxPath);
        } else {
            assert_eq!(storage_type, StorageType::DesktopPath);
        }
    }
}
