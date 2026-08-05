use tauri::{AppHandle, State};

use super::{CommandExecutionRuntime, CommandU64};
use crate::application::{document_query_service, editor_command_service};
use crate::error::AppError;
use crate::protocol_projection;
use crate::runtime::ApplicationRuntime;
use crate::types::{EditorMutationResponse, ImageAnchor, ImageSelection, SheetImagePage};

#[tauri::command]
pub async fn pick_image(
    app: AppHandle,
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
) -> Result<Option<ImageSelection>, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .file()
        .run(move || {
            let Some(selected) = crate::adapters::image_picker_adapter::pick_image_file(&app)?
            else {
                return Ok(None);
            };
            let staged = runtime.images().stage(selected.file_name, selected.bytes)?;
            Ok(Some(ImageSelection {
                token: staged.token,
                file_name: staged.file_name,
                mime_type: staged.mime_type,
                width: staged.width,
                height: staged.height,
            }))
        })
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn discard_image_selection(
    runtime: State<'_, ApplicationRuntime>,
    token: String,
) -> Result<(), AppError> {
    runtime.images().discard(&token)
}

#[tauri::command(rename_all = "camelCase")]
#[allow(clippy::too_many_arguments)]
pub async fn insert_image(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    row: u32,
    col: u32,
    selection_token: String,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    let result = executions
        .mutation()
        .run({
            let runtime = runtime.clone();
            let token = selection_token.clone();
            move || {
                let staged = runtime.images().get(&token)?;
                editor_command_service::insert_image(
                    runtime.editor_commands(),
                    document_id.get(),
                    base_revision.get(),
                    &command_id,
                    sheet_index,
                    row,
                    col,
                    staged,
                )
                .map(protocol_projection::mutation_response)
            }
        })
        .await;
    if result.is_ok() {
        runtime.images().discard(&selection_token)?;
    }
    result
}

#[tauri::command(rename_all = "camelCase")]
#[allow(clippy::too_many_arguments)]
pub async fn update_image(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    image_id: String,
    anchor: ImageAnchor,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .mutation()
        .run(move || {
            editor_command_service::update_image(
                runtime.editor_commands(),
                document_id.get(),
                base_revision.get(),
                &command_id,
                sheet_index,
                image_id,
                protocol_projection::domain_image_anchor(anchor),
            )
            .map(protocol_projection::mutation_response)
        })
        .await
}

#[tauri::command(rename_all = "camelCase")]
#[allow(clippy::too_many_arguments)]
pub async fn delete_image(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    image_id: String,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .mutation()
        .run(move || {
            editor_command_service::delete_image(
                runtime.editor_commands(),
                document_id.get(),
                base_revision.get(),
                &command_id,
                sheet_index,
                image_id,
            )
            .map(protocol_projection::mutation_response)
        })
        .await
}

#[tauri::command(rename_all = "camelCase")]
#[allow(clippy::too_many_arguments)]
pub async fn get_sheet_images(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    sheet_index: usize,
    offset: usize,
    limit: usize,
) -> Result<SheetImagePage, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .query()
        .run(move || {
            let (items, next_offset) = document_query_service::sheet_images_for_command(
                runtime.document_queries(),
                document_id.get(),
                base_revision.get(),
                sheet_index,
                offset,
                limit,
            )?;
            Ok(SheetImagePage {
                items: items
                    .into_iter()
                    .map(protocol_projection::sheet_image)
                    .collect(),
                next_offset,
            })
        })
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_image_bytes(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    sheet_index: usize,
    image_id: String,
) -> Result<tauri::ipc::Response, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .query()
        .run(move || {
            let bytes = document_query_service::image_bytes_for_command(
                runtime.document_queries(),
                document_id.get(),
                base_revision.get(),
                sheet_index,
                &image_id,
            )?;
            Ok(tauri::ipc::Response::new(bytes.as_ref().to_vec()))
        })
        .await
}
