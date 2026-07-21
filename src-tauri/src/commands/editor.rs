#![allow(clippy::needless_pass_by_value)]

use super::input::{BoundedCellText, SetCellBatch};
use super::{CommandExecutionRuntime, CommandU64};
use crate::application::editor_command_service;
use crate::application::runtime::ApplicationRuntime;
use crate::error::AppError;
use crate::protocol_projection;
use crate::types::{EditorMutationResponse, EditorSessionInfo};
use tauri::State;

#[tauri::command(rename_all = "camelCase")]
pub async fn get_editor_state(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: Option<CommandU64>,
    base_revision: Option<CommandU64>,
) -> Result<Option<EditorSessionInfo>, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .query()
        .run_mapped(
            move || {
                editor_command_service::get_editor_state(
                    runtime.editor_commands(),
                    document_id.map(CommandU64::get),
                    base_revision.map(CommandU64::get),
                )
            },
            |snapshot| snapshot.map(protocol_projection::editor_session),
        )
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn undo(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .mutation()
        .run_mapped(
            move || {
                editor_command_service::undo(
                    runtime.editor_commands(),
                    document_id.get(),
                    base_revision.get(),
                    &command_id,
                )
            },
            protocol_projection::mutation_response,
        )
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn redo(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .mutation()
        .run_mapped(
            move || {
                editor_command_service::redo(
                    runtime.editor_commands(),
                    document_id.get(),
                    base_revision.get(),
                    &command_id,
                )
            },
            protocol_projection::mutation_response,
        )
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_cell(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    row: usize,
    col: usize,
    text: BoundedCellText,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    let text = text.into_inner();
    executions
        .mutation()
        .run_mapped(
            move || {
                editor_command_service::set_cell(
                    runtime.editor_commands(),
                    document_id.get(),
                    base_revision.get(),
                    &command_id,
                    sheet_index,
                    row,
                    col,
                    text,
                )
            },
            protocol_projection::mutation_response,
        )
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_cells(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    changes: SetCellBatch,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    let changes = changes
        .into_inner()
        .into_iter()
        .map(|change| crate::domain::CellEditInput {
            sheet_index: change.sheet_index,
            row: change.row,
            col: change.col,
            text: change.text,
        })
        .collect();
    executions
        .mutation()
        .run_mapped(
            move || {
                editor_command_service::set_cells(
                    runtime.editor_commands(),
                    document_id.get(),
                    base_revision.get(),
                    &command_id,
                    changes,
                )
            },
            protocol_projection::mutation_response,
        )
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn add_row(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    row_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .mutation()
        .run_mapped(
            move || {
                editor_command_service::add_row(
                    runtime.editor_commands(),
                    document_id.get(),
                    base_revision.get(),
                    &command_id,
                    sheet_index,
                    row_index,
                )
            },
            protocol_projection::mutation_response,
        )
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_row(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    row_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .mutation()
        .run_mapped(
            move || {
                editor_command_service::delete_row(
                    runtime.editor_commands(),
                    document_id.get(),
                    base_revision.get(),
                    &command_id,
                    sheet_index,
                    row_index,
                )
            },
            protocol_projection::mutation_response,
        )
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn add_column(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    col_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .mutation()
        .run_mapped(
            move || {
                editor_command_service::add_column(
                    runtime.editor_commands(),
                    document_id.get(),
                    base_revision.get(),
                    &command_id,
                    sheet_index,
                    col_index,
                )
            },
            protocol_projection::mutation_response,
        )
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_column(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    col_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .mutation()
        .run_mapped(
            move || {
                editor_command_service::delete_column(
                    runtime.editor_commands(),
                    document_id.get(),
                    base_revision.get(),
                    &command_id,
                    sheet_index,
                    col_index,
                )
            },
            protocol_projection::mutation_response,
        )
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_column_width(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    col_index: usize,
    width: Option<u32>,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .mutation()
        .run_mapped(
            move || {
                editor_command_service::set_column_width(
                    runtime.editor_commands(),
                    document_id.get(),
                    base_revision.get(),
                    &command_id,
                    sheet_index,
                    col_index,
                    width,
                )
            },
            protocol_projection::mutation_response,
        )
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_row_height(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    row_index: usize,
    height: Option<u32>,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .mutation()
        .run_mapped(
            move || {
                editor_command_service::set_row_height(
                    runtime.editor_commands(),
                    document_id.get(),
                    base_revision.get(),
                    &command_id,
                    sheet_index,
                    row_index,
                    height,
                )
            },
            protocol_projection::mutation_response,
        )
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn add_sheet(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .mutation()
        .run_mapped(
            move || {
                editor_command_service::add_sheet(
                    runtime.editor_commands(),
                    document_id.get(),
                    base_revision.get(),
                    &command_id,
                )
            },
            protocol_projection::mutation_response,
        )
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_sheet(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .mutation()
        .run_mapped(
            move || {
                editor_command_service::delete_sheet(
                    runtime.editor_commands(),
                    document_id.get(),
                    base_revision.get(),
                    &command_id,
                    sheet_index,
                )
            },
            protocol_projection::mutation_response,
        )
        .await
}
