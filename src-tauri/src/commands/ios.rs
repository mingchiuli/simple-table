#[cfg(target_os = "ios")]
use super::{CommandU64, blocking};
#[cfg(target_os = "ios")]
use crate::error::AppError;
#[cfg(target_os = "ios")]
use crate::io::platform::mobile::PickedFileInfo;
#[cfg(target_os = "ios")]
use crate::io::platform::{ios, mobile};
#[cfg(target_os = "ios")]
use crate::io::transient_files::TransientFilePurpose;
#[cfg(target_os = "ios")]
use crate::types::{PreparedOpenDocument, SavedDocumentResponse};
#[cfg(target_os = "ios")]
use tauri::AppHandle;

/// iOS: import a picked file into the app sandbox without opening it in the editor.
#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn pick_open_file_ios(app: AppHandle) -> Result<Option<PickedFileInfo>, AppError> {
    blocking::run(move || ios::pick_file_info(&app)).await
}

/// iOS: remove a picked file that was imported but never opened as the active document.
#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn discard_open_file_selection_ios(app: AppHandle, path: String) -> Result<(), AppError> {
    blocking::run(move || {
        mobile::discard_transient_file(&app, &path, TransientFilePurpose::OpenSelection)
    })
    .await
}

/// iOS: remove a save-as target that was reserved but never adopted.
#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn discard_save_location_ios(app: AppHandle, path: String) -> Result<(), AppError> {
    blocking::run(move || {
        mobile::discard_transient_file(&app, &path, TransientFilePurpose::SaveLocation)
    })
    .await
}

/// iOS: read and parse a sandboxed file path saved in recent files.
#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn prepare_open_file_ios(
    app: AppHandle,
    path: String,
) -> Result<PreparedOpenDocument, AppError> {
    blocking::run(move || mobile::prepare_file(&app, &path)).await
}

/// iOS: create a new sandbox save target that must be adopted by save_file_ios or discarded.
#[cfg(target_os = "ios")]
#[tauri::command(rename_all = "camelCase")]
pub async fn pick_save_location_ios(
    app: AppHandle,
    default_name: String,
) -> Result<Option<String>, AppError> {
    blocking::run(move || Ok(Some(mobile::reserve_save_location(&app, &default_name)?))).await
}

/// iOS: generate file bytes and write them to the sandbox path.
#[cfg(target_os = "ios")]
#[tauri::command(rename_all = "camelCase")]
pub async fn save_file_ios(
    app: AppHandle,
    path: String,
    document_id: CommandU64,
    base_revision: CommandU64,
) -> Result<SavedDocumentResponse, AppError> {
    blocking::run(move || mobile::save_file(&app, &path, document_id.get(), base_revision.get()))
        .await
}

/// iOS: export a sandboxed file to a user-selected destination.
#[cfg(target_os = "ios")]
#[tauri::command(rename_all = "camelCase")]
pub async fn export_file_ios(
    app: AppHandle,
    default_name: String,
    document_id: CommandU64,
    base_revision: CommandU64,
) -> Result<Option<String>, AppError> {
    blocking::run(move || {
        ios::export_file(&app, &default_name, document_id.get(), base_revision.get())
    })
    .await
}
