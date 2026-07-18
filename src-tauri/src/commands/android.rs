#[cfg(target_os = "android")]
use super::{CommandU64, blocking};
#[cfg(target_os = "android")]
use crate::adapters::document_file_adapter;
#[cfg(target_os = "android")]
use crate::application::runtime::ApplicationRuntime;
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
use tauri::{AppHandle, State};

/// Android: import a picked file into the app sandbox without opening it in the editor.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn pick_open_file_android(
    runtime: State<'_, ApplicationRuntime>,
    app: AppHandle,
) -> Result<Option<PickedFileInfo>, AppError> {
    let runtime = runtime.inner().clone();
    blocking::run(move || android::pick_file_info(runtime.mobile_files(), &app)).await
}

/// Android: remove a picked file that was imported but never opened as the active document.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn discard_open_file_selection_android(
    runtime: State<'_, ApplicationRuntime>,
    app: AppHandle,
    path: String,
) -> Result<(), AppError> {
    let runtime = runtime.inner().clone();
    blocking::run(move || {
        mobile::discard_transient_file(
            runtime.mobile_files(),
            &app,
            &path,
            TransientFilePurpose::OpenSelection,
        )
    })
    .await
}

/// Android: remove a save-as target that was reserved but never adopted.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn discard_save_location_android(
    runtime: State<'_, ApplicationRuntime>,
    app: AppHandle,
    path: String,
) -> Result<(), AppError> {
    let runtime = runtime.inner().clone();
    blocking::run(move || {
        mobile::discard_transient_file(
            runtime.mobile_files(),
            &app,
            &path,
            TransientFilePurpose::SaveLocation,
        )
    })
    .await
}

/// Android: read and parse a sandboxed file path saved in recent files.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn prepare_open_file_android(
    runtime: State<'_, ApplicationRuntime>,
    app: AppHandle,
    path: String,
) -> Result<PreparedOpenDocument, AppError> {
    let runtime = runtime.inner().clone();
    blocking::run(move || {
        document_file_adapter::prepare_open_file_mobile(
            runtime.document_opens(),
            runtime.mobile_files(),
            &app,
            &path,
        )
    })
    .await
}

/// Android: generate file bytes and write them to the sandbox path.
#[cfg(target_os = "android")]
#[tauri::command(rename_all = "camelCase")]
pub async fn save_file_android(
    runtime: State<'_, ApplicationRuntime>,
    app: AppHandle,
    path: String,
    document_id: CommandU64,
    base_revision: CommandU64,
) -> Result<SavedDocumentResponse, AppError> {
    let runtime = runtime.inner().clone();
    blocking::run(move || {
        document_file_adapter::save_file_mobile(
            runtime.document_saves(),
            runtime.mobile_files(),
            &app,
            &path,
            document_id.get(),
            base_revision.get(),
        )
    })
    .await
}

/// Android: export a sandboxed file to a user-selected destination.
#[cfg(target_os = "android")]
#[tauri::command(rename_all = "camelCase")]
pub async fn export_file_android(
    runtime: State<'_, ApplicationRuntime>,
    app: AppHandle,
    default_name: String,
    document_id: CommandU64,
    base_revision: CommandU64,
) -> Result<Option<String>, AppError> {
    let runtime = runtime.inner().clone();
    blocking::run(move || {
        document_file_adapter::export_file_mobile(
            runtime.document_saves(),
            runtime.mobile_files(),
            &app,
            &default_name,
            document_id.get(),
            base_revision.get(),
        )
    })
    .await
}

/// Android: create a new sandbox path for a file that will be written by save_file_android.
#[cfg(target_os = "android")]
#[tauri::command(rename_all = "camelCase")]
pub async fn pick_save_location_android(
    runtime: State<'_, ApplicationRuntime>,
    app: AppHandle,
    default_name: String,
) -> Result<Option<String>, AppError> {
    let runtime = runtime.inner().clone();
    blocking::run(move || {
        Ok(Some(android::pick_save_location(
            runtime.mobile_files(),
            &app,
            &default_name,
        )?))
    })
    .await
}
