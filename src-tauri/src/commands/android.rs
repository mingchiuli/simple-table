#[cfg(target_os = "android")]
use super::{CommandU64, blocking};
#[cfg(target_os = "android")]
use crate::error::AppError;
#[cfg(target_os = "android")]
use crate::io::platform::mobile::PickedFileInfo;
#[cfg(target_os = "android")]
use crate::io::platform::{android, mobile};
#[cfg(target_os = "android")]
use crate::io::transient_files::TransientFilePurpose;
#[cfg(target_os = "android")]
use crate::types::{PreparedOpenDocument, SavedDocumentResponse};
#[cfg(target_os = "android")]
use tauri::AppHandle;

/// Android: import a picked file into the app sandbox without opening it in the editor.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn pick_open_file_android(app: AppHandle) -> Result<Option<PickedFileInfo>, AppError> {
    blocking::run(move || android::pick_file_info(&app)).await
}

/// Android: remove a picked file that was imported but never opened as the active document.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn discard_open_file_selection_android(
    app: AppHandle,
    path: String,
) -> Result<(), AppError> {
    blocking::run(move || {
        mobile::discard_transient_file(&app, &path, TransientFilePurpose::OpenSelection)
    })
    .await
}

/// Android: remove a save-as target that was reserved but never adopted.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn discard_save_location_android(app: AppHandle, path: String) -> Result<(), AppError> {
    blocking::run(move || {
        mobile::discard_transient_file(&app, &path, TransientFilePurpose::SaveLocation)
    })
    .await
}

/// Android: read and parse a sandboxed file path saved in recent files.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn prepare_open_file_android(
    app: AppHandle,
    path: String,
) -> Result<PreparedOpenDocument, AppError> {
    blocking::run(move || mobile::prepare_file(&app, &path)).await
}

/// Android: generate file bytes and write them to the sandbox path.
#[cfg(target_os = "android")]
#[tauri::command(rename_all = "camelCase")]
pub async fn save_file_android(
    app: AppHandle,
    path: String,
    document_id: CommandU64,
    base_revision: CommandU64,
) -> Result<SavedDocumentResponse, AppError> {
    blocking::run(move || mobile::save_file(&app, &path, document_id.get(), base_revision.get()))
        .await
}

/// Android: export a sandboxed file to a user-selected destination.
#[cfg(target_os = "android")]
#[tauri::command(rename_all = "camelCase")]
pub async fn export_file_android(
    app: AppHandle,
    default_name: String,
    document_id: CommandU64,
    base_revision: CommandU64,
) -> Result<Option<String>, AppError> {
    blocking::run(move || {
        mobile::export_file(&app, &default_name, document_id.get(), base_revision.get())
    })
    .await
}

/// Android: create a new sandbox path for a file that will be written by save_file_android.
#[cfg(target_os = "android")]
#[tauri::command(rename_all = "camelCase")]
pub async fn pick_save_location_android(
    app: AppHandle,
    default_name: String,
) -> Result<Option<String>, AppError> {
    blocking::run(move || Ok(Some(android::pick_save_location(&app, &default_name)?))).await
}
