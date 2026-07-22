#[cfg(target_os = "android")]
use super::{CommandExecutionRuntime, CommandU64};
#[cfg(target_os = "android")]
use crate::error::AppError;
#[cfg(target_os = "android")]
use crate::protocol_projection;
#[cfg(target_os = "android")]
use crate::runtime::ApplicationRuntime;
#[cfg(target_os = "android")]
use crate::types::{PickedFileInfo, PreparedOpenDocument, SavedDocumentResponse};
#[cfg(target_os = "android")]
use tauri::{AppHandle, State};

/// Android: import a picked file into the app sandbox without opening it in the editor.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn pick_open_file_android(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    app: AppHandle,
) -> Result<Option<PickedFileInfo>, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .file()
        .run_mapped(
            move || runtime.document_files().pick_open_file_android(&app),
            |selection| selection.map(protocol_projection::picked_file_info),
        )
        .await
}

/// Android: remove a picked file that was imported but never opened as the active document.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn discard_open_file_selection_android(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    app: AppHandle,
    path: String,
) -> Result<(), AppError> {
    let runtime = runtime.inner().clone();
    executions
        .file()
        .run(move || {
            runtime
                .document_files()
                .discard_open_file_selection_mobile(&app, &path)
        })
        .await
}

/// Android: remove a save-as target that was reserved but never adopted.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn discard_save_location_android(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    app: AppHandle,
    path: String,
) -> Result<(), AppError> {
    let runtime = runtime.inner().clone();
    executions
        .file()
        .run(move || {
            runtime
                .document_files()
                .discard_save_location_mobile(&app, &path)
        })
        .await
}

/// Android: read and parse a sandboxed file path saved in recent files.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn prepare_open_file_android(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    app: AppHandle,
    path: String,
) -> Result<PreparedOpenDocument, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .file()
        .run_mapped(
            move || {
                runtime
                    .document_files()
                    .prepare_open_file_mobile(&app, &path)
            },
            protocol_projection::prepared_open_document,
        )
        .await
}

/// Android: generate file bytes and write them to the sandbox path.
#[cfg(target_os = "android")]
#[tauri::command(rename_all = "camelCase")]
pub async fn save_file_android(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    app: AppHandle,
    path: String,
    document_id: CommandU64,
    base_revision: CommandU64,
) -> Result<SavedDocumentResponse, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .file()
        .run_fallibly_mapped(
            move || {
                runtime.document_files().save_file_mobile(
                    &app,
                    &path,
                    document_id.get(),
                    base_revision.get(),
                )
            },
            protocol_projection::saved_document_response,
        )
        .await
}

/// Android: export a sandboxed file to a user-selected destination.
#[cfg(target_os = "android")]
#[tauri::command(rename_all = "camelCase")]
pub async fn export_file_android(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    app: AppHandle,
    default_name: String,
    document_id: CommandU64,
    base_revision: CommandU64,
) -> Result<Option<String>, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .file()
        .run(move || {
            runtime.document_files().export_file_mobile(
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
    executions: State<'_, CommandExecutionRuntime>,
    app: AppHandle,
    default_name: String,
) -> Result<Option<String>, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .file()
        .run(move || {
            runtime
                .document_files()
                .pick_save_location_android(&app, &default_name)
        })
        .await
}
