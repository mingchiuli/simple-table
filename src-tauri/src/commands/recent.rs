use super::CommandExecutionRuntime;
use crate::adapters::recent_file_adapter;
use crate::error::AppError;
use crate::protocol_projection;
use crate::runtime::ApplicationRuntime;
use crate::types::{AddRecentFileRequest, RecentFile};
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn get_recent_files(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    app: AppHandle,
) -> Result<Vec<RecentFile>, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .recent()
        .run(move || {
            recent_file_adapter::do_get_recent_files(runtime.recent_files(), &app)
                .map(protocol_projection::recent_files)
        })
        .await
}

#[tauri::command]
pub async fn remove_recent_file(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    app: AppHandle,
    id: String,
) -> Result<(), AppError> {
    let runtime = runtime.inner().clone();
    executions
        .recent()
        .run(move || recent_file_adapter::do_remove_recent_file(runtime.recent_files(), &app, &id))
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn add_recent_file_with_thumbnail(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    app: AppHandle,
    request: AddRecentFileRequest,
) -> Result<RecentFile, AppError> {
    let runtime = runtime.inner().clone();
    let input = protocol_projection::add_recent_file_input(request);
    executions
        .recent()
        .run(move || {
            recent_file_adapter::do_add_recent_file_with_thumbnail(
                runtime.recent_files(),
                &app,
                input,
            )
            .map(protocol_projection::recent_file)
        })
        .await
}
