use tauri::AppHandle;

use crate::error::AppError;
use crate::io::document;

use super::store::RecentStore;
use super::thumbnail::{capture_thumbnail, generate_thumbnail};
use super::types::{AddRecentFileRequest, RecentFile, StorageType};

pub fn do_get_recent_files(app: &AppHandle) -> Result<Vec<RecentFile>, AppError> {
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
    let (path, file_name, thumbnail) =
        document::inspect_current_file_for_command(document_id, base_revision, |file_data| {
            (
                file_data.path.clone(),
                file_data.file_name.clone(),
                capture_thumbnail(file_data),
            )
        })?;

    if path.is_empty() {
        return Err(AppError::DocumentStateInvalid(
            "cannot add an unsaved document to recent files".to_string(),
        ));
    }

    let file_size = recent_file_size(&path);
    let mut recent_file = RecentFile::new(path, file_name, file_size);
    recent_file.storage_type = current_platform_storage_type();

    if let Some(op) = original_path {
        recent_file.original_path = Some(op);
    }

    recent_file.thumbnail = thumbnail.and_then(generate_thumbnail);

    let update = RecentStore::add(app, recent_file)?;
    cleanup_removed_mobile_files(app, &update.removed);
    Ok(update.updated)
}

pub fn do_remove_recent_file(app: &AppHandle, id: &str) -> Result<(), AppError> {
    let removed = RecentStore::remove(app, id)?;
    cleanup_removed_mobile_files(app, &removed);
    Ok(())
}

fn cleanup_removed_mobile_files(app: &AppHandle, removed: &[RecentFile]) {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    for file in removed {
        if file.storage_type != StorageType::MobileSandboxPath {
            continue;
        }
        if let Err(error) =
            crate::io::platform::mobile::remove_managed_file_if_inactive(app, &file.path)
        {
            eprintln!(
                "Failed to clean up removed mobile document {}: {error}",
                file.path
            );
        }
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let _ = (app, removed);
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
