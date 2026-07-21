#![allow(clippy::needless_pass_by_value)]

use super::{CommandExecutionRuntime, CommandU64};
use crate::application::runtime::ApplicationRuntime;
use crate::application::{document_query_service, document_service, editor_command_service};
use crate::error::AppError;
use crate::protocol_projection;
use crate::types::{
    DocumentCapabilities, MutationResultLookup, NativeSavePlan, OpenDocumentResponse, SheetRegion,
    SheetRegionProjectionResponse, SpreadsheetFormatOptions,
};
use tauri::State;

#[tauri::command]
pub async fn get_active_document(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
) -> Result<Option<OpenDocumentResponse>, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .projection()
        .run(move || document_query_service::active_document_response(runtime.document_queries()))
        .await
        .map(|snapshot| snapshot.map(protocol_projection::open_document_response))
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_mutation_result(
    runtime: State<'_, ApplicationRuntime>,
    document_id: CommandU64,
    command_id: String,
) -> Result<MutationResultLookup, AppError> {
    editor_command_service::get_mutation_result(
        runtime.editor_commands(),
        document_id.get(),
        &command_id,
    )
    .map(protocol_projection::mutation_lookup)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_current_document_projection(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    preferred_sheet_index: usize,
) -> Result<OpenDocumentResponse, AppError> {
    let runtime = runtime.inner().clone();
    executions
        .projection()
        .run(move || {
            document_query_service::current_document_projection_for_command(
                runtime.document_queries(),
                document_id.get(),
                base_revision.get(),
                preferred_sheet_index,
            )
        })
        .await
        .map(protocol_projection::open_document_response)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_sheet_region_projection(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    region: SheetRegion,
) -> Result<SheetRegionProjectionResponse, AppError> {
    let runtime = runtime.inner().clone();
    let snapshot = executions
        .projection()
        .run(move || {
            document_query_service::sheet_region_projection_for_command(
                runtime.document_queries(),
                document_id.get(),
                base_revision.get(),
                crate::document::region_metadata_index::DocumentRegion {
                    sheet_index: region.sheet_index,
                    row_start: region.row_start,
                    row_end: region.row_end,
                    col_start: region.col_start,
                    col_end: region.col_end,
                },
            )
        })
        .await?;
    protocol_projection::sheet_region_response(
        snapshot,
        crate::editor_protocol::MAX_SHEET_REGION_RESPONSE_BYTES,
    )
}

#[tauri::command(rename_all = "camelCase")]
pub async fn close_current_document(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
) -> Result<(), AppError> {
    let runtime = runtime.inner().clone();
    executions
        .mutation()
        .run(move || {
            document_service::close_current_document(
                runtime.document_lifecycle(),
                document_id.get(),
            )
        })
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_document_capabilities(
    runtime: State<'_, ApplicationRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
) -> Result<DocumentCapabilities, AppError> {
    document_query_service::document_capabilities_for_command(
        runtime.document_queries(),
        document_id.get(),
        base_revision.get(),
    )
    .map(protocol_projection::document_capabilities)
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_native_save_plan(
    runtime: State<'_, ApplicationRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    target_path_or_name: String,
) -> Result<NativeSavePlan, AppError> {
    document_query_service::native_save_plan_for_command(
        runtime.document_queries(),
        document_id.get(),
        base_revision.get(),
        &target_path_or_name,
    )
    .map(protocol_projection::native_save_plan)
}

#[tauri::command]
pub fn get_spreadsheet_format_options() -> SpreadsheetFormatOptions {
    protocol_projection::spreadsheet_format_options(document_query_service::format_options())
}
