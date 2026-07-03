#[cfg(target_os = "ios")]
use crate::error::AppError;
#[cfg(target_os = "ios")]
use crate::io::platform::mobile::{PickFileResult, PickedFileInfo};
#[cfg(target_os = "ios")]
use crate::io::platform::{ios, mobile};
#[cfg(target_os = "ios")]
use crate::types::OpenDocumentResponse;
#[cfg(target_os = "ios")]
use tauri::AppHandle;

/// iOS: use official dialog + fs plugins to import a picked file into the app sandbox.
#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn pick_file_ios(app: AppHandle) -> Result<Option<PickFileResult>, AppError> {
    ios::pick_file(&app)
}

/// iOS: read and parse a sandboxed file path saved in recent files.
#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn read_file_ios(app: AppHandle, path: String) -> Result<OpenDocumentResponse, AppError> {
    mobile::read_file(&app, &path)
}

/// iOS: create a new file in app sandbox.
#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn create_private_file_ios(
    app: AppHandle,
    file_name: String,
) -> Result<PickedFileInfo, AppError> {
    mobile::create_file(&app, &file_name)
}

/// iOS: generate file bytes and write them to the sandbox path.
#[cfg(target_os = "ios")]
#[tauri::command(rename_all = "camelCase")]
pub async fn save_file_ios(app: AppHandle, path: String) -> Result<(), AppError> {
    mobile::save_file(&app, &path)
}

/// iOS: export a sandboxed file to a user-selected destination.
#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn export_file_ios(
    app: AppHandle,
    source_path: String,
    default_name: String,
) -> Result<Option<String>, AppError> {
    ios::export_file(&app, &source_path, &default_name)
}
