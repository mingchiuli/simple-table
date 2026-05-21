#[cfg(target_os = "android")]
use crate::error::AppError;
#[cfg(target_os = "android")]
use crate::io::platform::mobile::PickFileResult;
#[cfg(target_os = "android")]
use crate::types::FileData;
#[cfg(target_os = "android")]
use tauri::AppHandle;

/// Android: use official dialog + fs plugins to import a picked file into the app sandbox.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn pick_file_android(app: AppHandle) -> Result<Option<PickFileResult>, AppError> {
    crate::io::platform::android::pick_file(&app)
}

/// Android: read and parse a sandboxed file path saved in recent files.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn read_file_android(app: AppHandle, path: String) -> Result<FileData, AppError> {
    crate::io::platform::mobile::read_file(&app, &path)
}

/// Android: generate file bytes and write them to the sandbox path.
#[cfg(target_os = "android")]
#[tauri::command(rename_all = "camelCase")]
pub async fn save_file_android(
    app: AppHandle,
    path: String,
    file_data: FileData,
) -> Result<(), AppError> {
    crate::io::platform::mobile::save_file(&app, &path, &file_data)
}

/// Android: export a sandboxed file to a user-selected destination.
#[cfg(target_os = "android")]
#[tauri::command(rename_all = "camelCase")]
pub async fn export_file_android(
    app: AppHandle,
    source_path: String,
    default_name: String,
) -> Result<Option<String>, AppError> {
    crate::io::platform::mobile::export_file(&app, &source_path, &default_name)
}

/// Android: create a new sandbox path for a file that will be written by save_file_android.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn pick_save_location_android(
    app: AppHandle,
    default_name: String,
) -> Result<Option<String>, AppError> {
    Ok(Some(crate::io::platform::android::pick_save_location(
        &app,
        &default_name,
    )?))
}
