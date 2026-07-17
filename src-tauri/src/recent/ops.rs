use tauri::AppHandle;

use crate::error::AppError;
use crate::io::document;

use super::store::RecentStore;
#[cfg(any(target_os = "android", target_os = "ios"))]
use super::store::validate_recent_files;
use super::thumbnail::{capture_thumbnail, generate_thumbnail};
use super::types::{AddRecentFileRequest, RecentFile, StorageType};

pub fn do_get_recent_files(app: &AppHandle) -> Result<Vec<RecentFile>, AppError> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let stored = match RecentStore::get_all(app) {
            Ok(files) => files,
            Err(error) => {
                eprintln!(
                    "Failed to read recent metadata; rebuilding from managed catalog: {error}"
                );
                Vec::new()
            }
        };
        return reconcile_mobile_recent_files(app, stored);
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
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

    #[cfg(any(target_os = "android", target_os = "ios"))]
    if let Some(managed) = crate::io::platform::mobile::managed_document_records(app)?
        .into_iter()
        .find(|managed| managed.path.to_string_lossy() == recent_file.path)
    {
        recent_file.id = managed.id;
    }

    if let Some(op) = original_path {
        recent_file.original_path = Some(op);
    }

    recent_file.thumbnail = thumbnail.and_then(generate_thumbnail);

    let update = RecentStore::add(app, recent_file)?;
    cleanup_removed_mobile_files(app, &update.removed);
    Ok(update.updated)
}

pub fn do_remove_recent_file(app: &AppHandle, id: &str) -> Result<(), AppError> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        if let Some(file) = do_get_recent_files(app)?
            .into_iter()
            .find(|file| file.id == id)
            && file.storage_type == StorageType::MobileSandboxPath
        {
            let active_path = document::active_document_path()?;
            if !crate::io::platform::mobile::remove_managed_file_if_inactive(
                app,
                &file.path,
                active_path.as_deref(),
            )? {
                return Err(AppError::DocumentStateInvalid(
                    "cannot delete the active mobile document".to_string(),
                ));
            }
        }
        RecentStore::remove(app, id)?;
        return Ok(());
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let removed = RecentStore::remove(app, id)?;
        cleanup_removed_mobile_files(app, &removed);
        Ok(())
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn reconcile_mobile_recent_files(
    app: &AppHandle,
    stored: Vec<RecentFile>,
) -> Result<Vec<RecentFile>, AppError> {
    use std::collections::HashMap;

    for file in stored
        .iter()
        .filter(|file| file.storage_type == StorageType::MobileSandboxPath)
    {
        if let Err(error) = crate::io::platform::mobile::migrate_managed_document(
            app,
            &file.path,
            &file.file_name,
            &file.id,
            file.last_opened,
        ) {
            eprintln!(
                "Failed to migrate managed mobile document {}: {error}",
                file.path
            );
        }
    }

    let mut stored_mobile: HashMap<_, _> = stored
        .iter()
        .filter(|file| file.storage_type == StorageType::MobileSandboxPath)
        .cloned()
        .map(|file| (file.path.clone(), file))
        .collect();
    let mut reconciled: Vec<_> = stored
        .into_iter()
        .filter(|file| file.storage_type != StorageType::MobileSandboxPath)
        .collect();

    for managed in crate::io::platform::mobile::managed_document_records(app)? {
        let path = managed.path.to_string_lossy().to_string();
        let existing = stored_mobile.remove(&path);
        reconciled.push(RecentFile {
            id: managed.id,
            path,
            file_name: managed.file_name,
            last_opened: existing
                .as_ref()
                .map(|file| file.last_opened)
                .unwrap_or(managed.adopted_at_millis),
            file_size: managed.file_size.min(i64::MAX as u64) as i64,
            thumbnail: existing.as_ref().and_then(|file| file.thumbnail.clone()),
            storage_type: StorageType::MobileSandboxPath,
            original_path: existing.and_then(|file| file.original_path),
        });
    }
    reconciled.sort_by_key(|file| std::cmp::Reverse(file.last_opened));
    let mut retained_thumbnails = 0;
    for file in &mut reconciled {
        if file.storage_type != StorageType::MobileSandboxPath {
            continue;
        }
        retained_thumbnails += 1;
        if retained_thumbnails > 10 {
            file.thumbnail = None;
        }
    }
    validate_recent_files(&reconciled).map_err(AppError::ReadError)?;
    if let Err(error) = RecentStore::replace_all(app, reconciled.clone()) {
        eprintln!("Failed to persist reconciled recent metadata: {error}");
    }
    Ok(reconciled)
}

fn cleanup_removed_mobile_files(app: &AppHandle, removed: &[RecentFile]) {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let active_path = match document::active_document_path() {
        Ok(active_path) => active_path,
        Err(error) => {
            eprintln!("Failed to inspect active document during recent cleanup: {error}");
            return;
        }
    };

    #[cfg(any(target_os = "android", target_os = "ios"))]
    for file in removed {
        if file.storage_type != StorageType::MobileSandboxPath {
            continue;
        }
        if let Err(error) = crate::io::platform::mobile::remove_managed_file_if_inactive(
            app,
            &file.path,
            active_path.as_deref(),
        ) {
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
            "documentId": "7",
            "baseRevision": "3",
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
