use crate::application::document_open_service::{self, DocumentOpenService};
use crate::application::document_save_service::{self, DocumentSaveService};
use crate::error::AppError;
use crate::types::{PreparedOpenDocument, SavedDocumentResponse};

#[cfg(desktop)]
pub fn prepare_open_file_desktop(
    service: &DocumentOpenService,
    files: &crate::io::platform::desktop::DesktopFileRuntime,
    path: &str,
) -> Result<PreparedOpenDocument, AppError> {
    document_open_service::prepare_open_input(
        service,
        crate::io::platform::desktop::read_open_file(files, path)?,
    )
}

#[cfg(desktop)]
pub fn prepare_recent_file_desktop(
    service: &DocumentOpenService,
    recent_files: &crate::recent::store::RecentStore,
    app: &tauri::AppHandle,
    id: &str,
) -> Result<PreparedOpenDocument, AppError> {
    document_open_service::prepare_open_input(
        service,
        crate::io::platform::desktop::read_recent_file(recent_files, app, id)?,
    )
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn prepare_open_file_mobile(
    service: &DocumentOpenService,
    files: &crate::io::platform::mobile::MobileFileRuntime,
    app: &tauri::AppHandle,
    path: &str,
) -> Result<PreparedOpenDocument, AppError> {
    document_open_service::prepare_open_input(
        service,
        crate::io::platform::mobile::read_open_file(files, app, path)?,
    )
}

#[cfg(desktop)]
pub fn save_file_desktop(
    service: &DocumentSaveService,
    files: &crate::io::platform::desktop::DesktopFileRuntime,
    path: &str,
    document_id: u64,
    base_revision: u64,
) -> Result<SavedDocumentResponse, AppError> {
    use std::path::Path;

    use crate::io::atomic_file::{
        cleanup_temp_file, replace_temp_file, write_temp_file_for_target,
    };
    use crate::io::platform::desktop;

    let current_path = document_save_service::current_document_path_for_command(
        service,
        document_id,
        base_revision,
    )?;
    desktop::ensure_save_path_authorized(files, path, &current_path)?;
    let prepared = document_save_service::prepare_current_file_save(
        service,
        document_id,
        base_revision,
        path,
    )?;
    let target = Path::new(path);
    let temp_path = match write_temp_file_for_target(target, &prepared.bytes) {
        Ok(temp_path) => temp_path,
        Err(error) => {
            document_save_service::abort_prepared_file_save(prepared);
            return Err(error);
        }
    };

    let result = document_save_service::commit_current_file_save(
        service,
        path.to_string(),
        prepared,
        || replace_temp_file(&temp_path, target),
    );
    if result.is_err() {
        cleanup_temp_file(&temp_path);
    }
    result
}

#[cfg(desktop)]
pub fn export_file_desktop(
    service: &DocumentSaveService,
    app: &tauri::AppHandle,
    default_name: &str,
    document_id: u64,
    base_revision: u64,
) -> Result<Option<String>, AppError> {
    use crate::io::platform::desktop;

    let Some(target) = desktop::pick_export_target(app, default_name)? else {
        return Ok(None);
    };
    let prepared = document_save_service::prepare_current_file_export(
        service,
        document_id,
        base_revision,
        &target.target_path_or_name,
    )?;
    desktop::write_export_target(&target, &prepared.bytes)?;
    Ok(Some(target.path_string))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn save_file_mobile(
    service: &DocumentSaveService,
    files: &crate::io::platform::mobile::MobileFileRuntime,
    app: &tauri::AppHandle,
    path: &str,
    document_id: u64,
    base_revision: u64,
) -> Result<SavedDocumentResponse, AppError> {
    use std::path::Path;

    use crate::io::atomic_file::{
        cleanup_temp_file, replace_temp_file, write_temp_file_for_target,
    };
    use crate::io::managed_documents;
    use crate::io::platform::mobile;

    let target = mobile::validated_mobile_files_path(files, app, Path::new(path))?;
    let current_path = document_save_service::current_document_path_for_command(
        service,
        document_id,
        base_revision,
    )?;
    mobile::ensure_save_target_authorized(files, &target, &current_path)?;
    let target_path = target.to_string_lossy().to_string();
    let prepared = document_save_service::prepare_current_file_save(
        service,
        document_id,
        base_revision,
        &target_path,
    )?;
    managed_documents::validate_managed_save(
        files.managed_documents(),
        &target,
        prepared.bytes.len() as u64,
    )?;
    let temp_path = match write_temp_file_for_target(&target, &prepared.bytes) {
        Ok(temp_path) => temp_path,
        Err(error) => {
            document_save_service::abort_prepared_file_save(prepared);
            return Err(error);
        }
    };

    let managed_file_name = prepared.output_name.clone();
    let result =
        document_save_service::commit_current_file_save(service, target_path, prepared, || {
            replace_temp_file(&temp_path, &target)?;
            managed_documents::adopt_completed_save(
                files.managed_documents(),
                files.transient_files(),
                &target,
                &managed_file_name,
            )
        });
    if result.is_err() {
        cleanup_temp_file(&temp_path);
    }
    result
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn export_file_mobile(
    service: &DocumentSaveService,
    files: &crate::io::platform::mobile::MobileFileRuntime,
    app: &tauri::AppHandle,
    default_name: &str,
    document_id: u64,
    base_revision: u64,
) -> Result<Option<String>, AppError> {
    use crate::io::platform::mobile;

    let Some(target) = mobile::pick_export_target(app, default_name)? else {
        return Ok(None);
    };
    let prepared = document_save_service::prepare_current_file_export(
        service,
        document_id,
        base_revision,
        &target.target_path_or_name,
    )?;
    mobile::write_export_target(app, &target, &prepared.bytes)?;
    Ok(Some(target.destination_string))
}
