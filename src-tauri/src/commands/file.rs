#![allow(clippy::needless_pass_by_value)]

use super::{CommandExecutionRuntime, CommandU64};
use crate::application::{document_open_service, document_service};
use crate::error::AppError;
use crate::protocol_projection;
use crate::runtime::ApplicationRuntime;
use crate::types::{
    DesktopOpenFileInfo, FileOperationReceipt, FileOperationResultLookup, PreparedOpenDocument,
    SavedDocumentResponse,
};
#[cfg(desktop)]
use tauri::Emitter;
use tauri::{AppHandle, State};

/// Desktop: 后端选择文件路径并授权随后读取。
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub async fn pick_open_file_desktop(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    app: AppHandle,
) -> Result<Option<DesktopOpenFileInfo>, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .file()
        .run_mapped(
            move || runtime.platform_files().pick_open_file(&app),
            |selection| selection.map(protocol_projection::desktop_open_file_info),
        )
        .await
}

/// Desktop: 释放已选择但没有被读取的文件路径授权。
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub fn discard_open_file_selection_desktop(runtime: State<'_, ApplicationRuntime>, path: String) {
    runtime.platform_files().discard_open_file_selection(&path)
}

/// Desktop: claim one normalized launch/file-association target authorized by the backend.
#[cfg(desktop)]
#[tauri::command]
pub fn claim_pending_open_target_desktop(
    runtime: State<'_, ApplicationRuntime>,
) -> Result<Option<crate::types::DesktopOpenTargetClaim>, AppError> {
    Ok(runtime
        .platform_files()
        .claim_pending_open_target()?
        .map(protocol_projection::desktop_open_target_claim))
}

#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub fn acknowledge_open_target_desktop(
    runtime: State<'_, ApplicationRuntime>,
    app: AppHandle,
    claim_id: String,
) -> Result<(), AppError> {
    if runtime
        .platform_files()
        .acknowledge_open_target(&claim_id)?
    {
        app.emit("deep-link-received", ()).map_err(|error| {
            AppError::Internal(format!("Failed to emit launch target wake event: {error}"))
        })?;
    }
    Ok(())
}

#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub fn release_open_target_desktop(
    runtime: State<'_, ApplicationRuntime>,
    claim_id: String,
) -> Result<(), AppError> {
    runtime.platform_files().release_open_target(&claim_id)
}

/// Desktop: 从后端已授权路径读取并解析文件。
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub async fn prepare_open_file_desktop(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    path: String,
) -> Result<PreparedOpenDocument, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .file()
        .run(move || {
            let source = runtime.platform_files().open_source(path);
            runtime
                .document_files()
                .prepare_open_projected(source, protocol_projection::prepared_open_document)
        })
        .await
}

/// Desktop: 通过最近文件 id 读取后端 recent store 中的路径。
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub async fn prepare_recent_file_desktop(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    app: AppHandle,
    id: String,
) -> Result<PreparedOpenDocument, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .file()
        .run(move || {
            let source = runtime.platform_files().recent_open_source(app, id);
            runtime
                .document_files()
                .prepare_open_projected(source, protocol_projection::prepared_open_document)
        })
        .await
}

/// Desktop: 后端选择保存路径并授权随后保存。
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub async fn pick_save_location_desktop(
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
                .platform_files()
                .pick_save_location(&app, &default_name)
        })
        .await
}

/// Desktop: 释放未使用的保存路径授权。
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub fn discard_save_location_desktop(runtime: State<'_, ApplicationRuntime>, path: String) {
    runtime.platform_files().discard_save_location(&path)
}

/// Desktop: 生成文件字节并写入路径
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub async fn save_file_desktop(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    path: String,
    document_id: CommandU64,
    base_revision: CommandU64,
    operation_id: String,
) -> Result<SavedDocumentResponse, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .file()
        .run(move || {
            let target = runtime.platform_files().save_target(path);
            runtime.document_files().save_projected(
                target,
                document_id.get(),
                base_revision.get(),
                &operation_id,
                protocol_projection::saved_document_response,
            )
        })
        .await
}

/// Desktop: 导出当前内容到指定路径，不改变当前编辑文档身份。
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub async fn export_file_desktop(
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
            let Some(target) = runtime
                .platform_files()
                .pick_export_target(&app, &default_name)?
            else {
                return Ok(None);
            };
            runtime
                .document_files()
                .export(target, document_id.get(), base_revision.get())
                .map(Some)
        })
        .await
}

#[tauri::command]
pub async fn prepare_new_file(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
) -> Result<PreparedOpenDocument, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .file()
        .run(move || {
            runtime
                .document_files()
                .prepare_new_projected(protocol_projection::prepared_open_document)
        })
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn commit_prepared_document(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    token: String,
    expected_document_id: Option<CommandU64>,
    expected_revision: Option<CommandU64>,
    operation_id: String,
) -> Result<FileOperationReceipt, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .mutation()
        .run_mapped(
            move || {
                document_service::commit_prepared_document(
                    runtime.document_lifecycle(),
                    &token,
                    expected_document_id.map(CommandU64::get),
                    expected_revision.map(CommandU64::get),
                    &operation_id,
                )
            },
            protocol_projection::file_operation_receipt,
        )
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_file_operation_result(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    operation_id: String,
) -> Result<FileOperationResultLookup, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .query()
        .run_mapped(
            move || {
                runtime
                    .document_files()
                    .file_operation_result(&operation_id)
            },
            protocol_projection::file_operation_lookup,
        )
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn abort_prepared_document(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    token: String,
) -> Result<(), AppError> {
    let runtime = runtime.inner().clone();
    executions
        .file()
        .run(move || {
            document_open_service::abort_prepared_document(runtime.document_opens(), &token)
        })
        .await
}
