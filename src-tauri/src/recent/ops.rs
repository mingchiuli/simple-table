use tauri::AppHandle;

use crate::error::AppError;
use crate::io::document;

use super::store::RecentStore;
use super::thumbnail::generate_thumbnail_from_file_data;
use super::types::{AddRecentFileRequest, RecentFile};

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

    if let Some(storage_type) = storage_type {
        recent_file.storage_type = storage_type;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recent::types::StorageType;
    use serde_json::json;

    #[test]
    fn add_recent_request_uses_generated_storage_type_contract() {
        let request: AddRecentFileRequest = serde_json::from_value(json!({
            "path": "/tmp/book.xlsx",
            "fileName": "book.xlsx",
            "fileSize": 42,
            "storageType": "mobileSandboxPath"
        }))
        .expect("recent request");

        assert_eq!(request.storage_type, Some(StorageType::MobileSandboxPath));
    }

    #[test]
    fn add_recent_request_rejects_unknown_storage_type() {
        let error = serde_json::from_value::<AddRecentFileRequest>(json!({
            "path": "/tmp/book.xlsx",
            "fileName": "book.xlsx",
            "fileSize": 42,
            "storageType": "unknown"
        }))
        .expect_err("unknown storage type should fail deserialization");

        assert!(error.to_string().contains("unknown variant"));
    }
}
