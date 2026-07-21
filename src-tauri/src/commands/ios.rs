#[cfg(target_os = "ios")]
use super::{CommandExecutionRuntime, CommandU64};
#[cfg(target_os = "ios")]
use crate::application::runtime::ApplicationRuntime;
#[cfg(target_os = "ios")]
use crate::error::AppError;
#[cfg(target_os = "ios")]
use crate::protocol_projection;
#[cfg(target_os = "ios")]
use crate::types::{PickedFileInfo, PreparedOpenDocument, SavedDocumentResponse};
#[cfg(target_os = "ios")]
use tauri::{AppHandle, State};

/// iOS: import a picked file into the app sandbox without opening it in the editor.
#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn pick_open_file_ios(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    app: AppHandle,
) -> Result<Option<PickedFileInfo>, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .file()
        .run_mapped(
            move || runtime.document_files().pick_open_file_ios(&app),
            |selection| selection.map(protocol_projection::picked_file_info),
        )
        .await
}

/// iOS: remove a picked file that was imported but never opened as the active document.
#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn discard_open_file_selection_ios(
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

/// iOS: remove a save-as target that was reserved but never adopted.
#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn discard_save_location_ios(
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

/// iOS: read and parse a sandboxed file path saved in recent files.
#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn prepare_open_file_ios(
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

/// iOS: create a new sandbox save target that must be adopted by save_file_ios or discarded.
#[cfg(target_os = "ios")]
#[tauri::command(rename_all = "camelCase")]
pub async fn pick_save_location_ios(
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
                .pick_save_location_ios(&app, &default_name)
        })
        .await
}

/// iOS: generate file bytes and write them to the sandbox path.
#[cfg(target_os = "ios")]
#[tauri::command(rename_all = "camelCase")]
pub async fn save_file_ios(
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

/// iOS: export a sandboxed file to a user-selected destination.
#[cfg(target_os = "ios")]
#[tauri::command(rename_all = "camelCase")]
pub async fn export_file_ios(
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
