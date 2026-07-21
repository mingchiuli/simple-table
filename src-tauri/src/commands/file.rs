#![allow(clippy::needless_pass_by_value)]

use super::{CommandExecutionRuntime, CommandU64};
use crate::application::runtime::ApplicationRuntime;
use crate::application::{document_open_service, document_service};
use crate::error::AppError;
use crate::protocol_projection;
use crate::types::{
    DesktopOpenFileInfo, OpenDocumentResponse, PreparedOpenDocument, SavedDocumentResponse,
};
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
        .run(move || runtime.document_files().pick_open_file(&app))
        .await
        .map(|selection| selection.map(protocol_projection::desktop_open_file_info))
}

/// Desktop: 释放已选择但没有被读取的文件路径授权。
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub fn discard_open_file_selection_desktop(runtime: State<'_, ApplicationRuntime>, path: String) {
    runtime.document_files().discard_open_file_selection(&path)
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
        .run(move || runtime.document_files().prepare_open_file(&path))
        .await
        .map(protocol_projection::prepared_open_document)
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
        .run(move || runtime.document_files().prepare_recent_file(&app, &id))
        .await
        .map(protocol_projection::prepared_open_document)
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
                .document_files()
                .pick_save_location(&app, &default_name)
        })
        .await
}

/// Desktop: 释放未使用的保存路径授权。
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub fn discard_save_location_desktop(runtime: State<'_, ApplicationRuntime>, path: String) {
    runtime.document_files().discard_save_location(&path)
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
) -> Result<SavedDocumentResponse, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .file()
        .run(move || {
            runtime
                .document_files()
                .save_file(&path, document_id.get(), base_revision.get())
        })
        .await
        .map(protocol_projection::saved_document_response)
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
            runtime.document_files().export_file(
                &app,
                &default_name,
                document_id.get(),
                base_revision.get(),
            )
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
        .run(move || document_open_service::prepare_new_file(runtime.document_opens()))
        .await
        .map(protocol_projection::prepared_open_document)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn commit_prepared_document(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    token: String,
    expected_document_id: Option<CommandU64>,
    expected_revision: Option<CommandU64>,
) -> Result<OpenDocumentResponse, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .mutation()
        .run(move || {
            document_service::commit_prepared_document(
                runtime.document_lifecycle(),
                &token,
                expected_document_id.map(CommandU64::get),
                expected_revision.map(CommandU64::get),
            )
        })
        .await
        .map(protocol_projection::open_document_response)
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
