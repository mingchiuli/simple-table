#[cfg(target_os = "ios")]
use crate::error::AppError;
#[cfg(target_os = "ios")]
use crate::io::platform::mobile::{PickFileResult, PickedFileInfo};
#[cfg(target_os = "ios")]
use crate::types::FileData;
#[cfg(target_os = "ios")]
use tauri::AppHandle;

/// iOS: use official dialog + fs plugins to import a picked file into the app sandbox.
#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn pick_file_ios(app: AppHandle) -> Result<Option<PickFileResult>, AppError> {
    crate::io::platform::ios::pick_file(&app)
}

/// iOS: read and parse a sandboxed file path saved in recent files.
#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn read_file_ios(app: AppHandle, path: String) -> Result<FileData, AppError> {
    crate::io::platform::mobile::read_file(&app, &path)
}

/// iOS: create a new file in app sandbox.
#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn create_private_file_ios(
    app: AppHandle,
    file_name: String,
) -> Result<PickedFileInfo, AppError> {
    crate::io::platform::mobile::create_file(&app, &file_name)
}

/// iOS: generate file bytes and write them to the sandbox path.
#[cfg(target_os = "ios")]
#[tauri::command(rename_all = "camelCase")]
pub async fn save_file_ios(
    app: AppHandle,
    path: String,
    file_data: FileData,
) -> Result<(), AppError> {
    crate::io::platform::mobile::save_file(&app, &path, &file_data)
}

/// iOS: export a sandboxed file to a user-selected destination.
#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn export_file_ios(
    app: AppHandle,
    source_path: String,
    default_name: String,
) -> Result<Option<String>, AppError> {
    crate::io::platform::ios::export_file(&app, &source_path, &default_name)
}
