#![allow(clippy::needless_pass_by_value)]

use super::{CommandU64, blocking, mutation_executor, mutation_replay, projection_executor};
use crate::error::AppError;
use crate::io::document;
#[cfg(desktop)]
use crate::io::platform::desktop;
use crate::ops::{cell_ops, editor_ops, search_ops};
use crate::recent::{self, AddRecentFileRequest, RecentFile};
use crate::state::{active_document_store, state::EditorSessionInfo};
#[cfg(desktop)]
use crate::types::SavedDocumentResponse;
use crate::types::{
    DocumentCapabilities, EditorMutationResponse, FileData, NativeSavePlan, OpenDocumentResponse,
    PreparedOpenDocument, SearchResult, SearchScope, SetCellRequest, SheetRegion,
    SheetRegionProjectionResponse, SpreadsheetFormatOptions,
};
use tauri::AppHandle;

const MAX_SET_CELL_CHANGES: usize = 4_096;

#[derive(Debug)]
pub(crate) struct SetCellBatch(Vec<SetCellRequest>);

impl SetCellBatch {
    fn into_inner(self) -> Vec<SetCellRequest> {
        self.0
    }
}

impl<'de> serde::Deserialize<'de> for SetCellBatch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct BatchVisitor;

        impl<'de> serde::de::Visitor<'de> for BatchVisitor {
            type Value = SetCellBatch;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(
                    formatter,
                    "an array containing at most {MAX_SET_CELL_CHANGES} cell changes"
                )
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut changes = Vec::with_capacity(
                    sequence
                        .size_hint()
                        .unwrap_or_default()
                        .min(MAX_SET_CELL_CHANGES),
                );
                while let Some(change) = sequence.next_element()? {
                    if changes.len() == MAX_SET_CELL_CHANGES {
                        return Err(serde::de::Error::custom(format!(
                            "set_cells accepts at most {MAX_SET_CELL_CHANGES} changes"
                        )));
                    }
                    changes.push(change);
                }
                Ok(SetCellBatch(changes))
            }
        }

        deserializer.deserialize_seq(BatchVisitor)
    }
}

// ==================== File Operations ====================

/// Desktop: 后端选择文件路径并授权随后读取。
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub async fn pick_open_file_desktop(
    app: AppHandle,
) -> Result<Option<desktop::DesktopOpenFileInfo>, AppError> {
    blocking::run(move || desktop::pick_open_file(&app)).await
}

/// Desktop: 释放已选择但没有被读取的文件路径授权。
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub fn discard_open_file_selection_desktop(path: String) {
    desktop::discard_open_file_selection(&path)
}

/// Desktop: 从后端已授权路径读取并解析文件。
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub async fn prepare_open_file_desktop(path: String) -> Result<PreparedOpenDocument, AppError> {
    blocking::run(move || desktop::prepare_file(&path)).await
}

/// Desktop: 通过最近文件 id 读取后端 recent store 中的路径。
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub async fn prepare_recent_file_desktop(
    app: AppHandle,
    id: String,
) -> Result<PreparedOpenDocument, AppError> {
    blocking::run(move || desktop::prepare_recent_file(&app, &id)).await
}

/// Desktop: 后端选择保存路径并授权随后保存。
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub async fn pick_save_location_desktop(
    app: AppHandle,
    default_name: String,
) -> Result<Option<String>, AppError> {
    blocking::run(move || desktop::pick_save_location(&app, &default_name)).await
}

/// Desktop: 释放未使用的保存路径授权。
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub fn discard_save_location_desktop(path: String) {
    desktop::discard_save_location(&path)
}

/// Desktop: 生成文件字节并写入路径
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub async fn save_file_desktop(
    path: String,
    document_id: CommandU64,
    base_revision: CommandU64,
) -> Result<SavedDocumentResponse, AppError> {
    blocking::run(move || desktop::save_file(&path, document_id.get(), base_revision.get())).await
}

/// Desktop: 导出当前内容到指定路径，不改变当前编辑文档身份。
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub async fn export_file_desktop(
    app: AppHandle,
    default_name: String,
    document_id: CommandU64,
    base_revision: CommandU64,
) -> Result<Option<String>, AppError> {
    blocking::run(move || {
        desktop::export_file(&app, &default_name, document_id.get(), base_revision.get())
    })
    .await
}

#[tauri::command]
pub async fn prepare_new_file(file_data: FileData) -> Result<PreparedOpenDocument, AppError> {
    blocking::run(move || document::prepare_new_file(file_data)).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn commit_prepared_document(
    token: String,
    expected_document_id: Option<CommandU64>,
    expected_revision: Option<CommandU64>,
) -> Result<OpenDocumentResponse, AppError> {
    mutation_executor::run(move || {
        document::commit_prepared_document(
            &token,
            expected_document_id.map(CommandU64::get),
            expected_revision.map(CommandU64::get),
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub fn abort_prepared_document(token: String) -> Result<(), AppError> {
    document::abort_prepared_document(&token)
}

#[tauri::command]
pub async fn get_active_document() -> Result<Option<OpenDocumentResponse>, AppError> {
    projection_executor::run(document::active_document_response).await
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_mutation_result(
    document_id: CommandU64,
    command_id: String,
) -> Result<Option<EditorMutationResponse>, AppError> {
    mutation_replay::get(document_id.get(), &command_id)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_current_document_projection(
    document_id: CommandU64,
    base_revision: CommandU64,
    preferred_sheet_index: usize,
) -> Result<OpenDocumentResponse, AppError> {
    projection_executor::run(move || {
        document::current_document_projection_for_command(
            document_id.get(),
            base_revision.get(),
            preferred_sheet_index,
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_sheet_region_projection(
    document_id: CommandU64,
    base_revision: CommandU64,
    region: SheetRegion,
) -> Result<SheetRegionProjectionResponse, AppError> {
    projection_executor::run(move || {
        document::sheet_region_projection_for_command(
            document_id.get(),
            base_revision.get(),
            region,
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub fn close_current_document(document_id: CommandU64) -> Result<(), AppError> {
    document::close_current_document(document_id.get())
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_document_capabilities(
    document_id: CommandU64,
    base_revision: CommandU64,
) -> Result<DocumentCapabilities, AppError> {
    document::document_capabilities_for_command(document_id.get(), base_revision.get())
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_native_save_plan(
    document_id: CommandU64,
    base_revision: CommandU64,
    target_path_or_name: String,
) -> Result<NativeSavePlan, AppError> {
    document::native_save_plan_for_command(
        document_id.get(),
        base_revision.get(),
        &target_path_or_name,
    )
}

#[tauri::command]
pub fn get_spreadsheet_format_options() -> SpreadsheetFormatOptions {
    document::format_options()
}

// ==================== Editor Operations ====================

#[tauri::command(rename_all = "camelCase")]
pub fn get_editor_state(
    document_id: Option<CommandU64>,
    base_revision: Option<CommandU64>,
) -> Result<Option<EditorSessionInfo>, AppError> {
    let registry = active_document_store();
    editor_ops::do_get_editor_state(
        &registry,
        document_id.map(CommandU64::get),
        base_revision.map(CommandU64::get),
    )
}

#[tauri::command(rename_all = "camelCase")]
pub async fn undo(
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    mutation_executor::run(move || {
        mutation_replay::run(
            document_id.get(),
            base_revision.get(),
            &command_id,
            "undo",
            &(),
            || editor_ops::do_undo(&registry, document_id.get(), base_revision.get()),
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn redo(
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    mutation_executor::run(move || {
        mutation_replay::run(
            document_id.get(),
            base_revision.get(),
            &command_id,
            "redo",
            &(),
            || editor_ops::do_redo(&registry, document_id.get(), base_revision.get()),
        )
    })
    .await
}

// ==================== Cell Operations ====================

#[tauri::command(rename_all = "camelCase")]
pub async fn set_cell(
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    row: usize,
    col: usize,
    text: String,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    mutation_executor::run(move || {
        mutation_replay::run(
            document_id.get(),
            base_revision.get(),
            &command_id,
            "set_cell",
            &(sheet_index, row, col, &text),
            || {
                cell_ops::do_set_cell(
                    &registry,
                    document_id.get(),
                    base_revision.get(),
                    sheet_index,
                    row,
                    col,
                    text.clone(),
                )
            },
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_cells(
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    changes: SetCellBatch,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    let changes = changes.into_inner();
    mutation_executor::run(move || {
        mutation_replay::run(
            document_id.get(),
            base_revision.get(),
            &command_id,
            "set_cells",
            &changes,
            || {
                cell_ops::do_set_cells(
                    &registry,
                    document_id.get(),
                    base_revision.get(),
                    changes.clone(),
                )
            },
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn add_row(
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    row_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    mutation_executor::run(move || {
        mutation_replay::run(
            document_id.get(),
            base_revision.get(),
            &command_id,
            "add_row",
            &(sheet_index, row_index),
            || {
                cell_ops::do_add_row(
                    &registry,
                    document_id.get(),
                    base_revision.get(),
                    sheet_index,
                    row_index,
                )
            },
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_row(
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    row_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    mutation_executor::run(move || {
        mutation_replay::run(
            document_id.get(),
            base_revision.get(),
            &command_id,
            "delete_row",
            &(sheet_index, row_index),
            || {
                cell_ops::do_delete_row(
                    &registry,
                    document_id.get(),
                    base_revision.get(),
                    sheet_index,
                    row_index,
                )
            },
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn add_column(
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    col_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    mutation_executor::run(move || {
        mutation_replay::run(
            document_id.get(),
            base_revision.get(),
            &command_id,
            "add_column",
            &(sheet_index, col_index),
            || {
                cell_ops::do_add_column(
                    &registry,
                    document_id.get(),
                    base_revision.get(),
                    sheet_index,
                    col_index,
                )
            },
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_column(
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    col_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    mutation_executor::run(move || {
        mutation_replay::run(
            document_id.get(),
            base_revision.get(),
            &command_id,
            "delete_column",
            &(sheet_index, col_index),
            || {
                cell_ops::do_delete_column(
                    &registry,
                    document_id.get(),
                    base_revision.get(),
                    sheet_index,
                    col_index,
                )
            },
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_column_width(
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    col_index: usize,
    width: Option<u32>,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    mutation_executor::run(move || {
        mutation_replay::run(
            document_id.get(),
            base_revision.get(),
            &command_id,
            "set_column_width",
            &(sheet_index, col_index, width),
            || {
                cell_ops::do_set_column_width(
                    &registry,
                    document_id.get(),
                    base_revision.get(),
                    sheet_index,
                    col_index,
                    width,
                )
            },
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_row_height(
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    row_index: usize,
    height: Option<u32>,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    mutation_executor::run(move || {
        mutation_replay::run(
            document_id.get(),
            base_revision.get(),
            &command_id,
            "set_row_height",
            &(sheet_index, row_index, height),
            || {
                cell_ops::do_set_row_height(
                    &registry,
                    document_id.get(),
                    base_revision.get(),
                    sheet_index,
                    row_index,
                    height,
                )
            },
        )
    })
    .await
}

// ==================== Sheet Operations ====================

#[tauri::command(rename_all = "camelCase")]
pub async fn add_sheet(
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    mutation_executor::run(move || {
        mutation_replay::run(
            document_id.get(),
            base_revision.get(),
            &command_id,
            "add_sheet",
            &(),
            || cell_ops::do_add_sheet(&registry, document_id.get(), base_revision.get()),
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_sheet(
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    mutation_executor::run(move || {
        mutation_replay::run(
            document_id.get(),
            base_revision.get(),
            &command_id,
            "delete_sheet",
            &sheet_index,
            || {
                cell_ops::do_delete_sheet(
                    &registry,
                    document_id.get(),
                    base_revision.get(),
                    sheet_index,
                )
            },
        )
    })
    .await
}

// ==================== Search Operations ====================

#[tauri::command(rename_all = "camelCase")]
pub async fn search(
    document_id: CommandU64,
    base_revision: CommandU64,
    query: String,
    scope: SearchScope,
    current_sheet_index: Option<usize>,
) -> Result<Vec<SearchResult>, AppError> {
    let registry = active_document_store();
    blocking::run(move || {
        search_ops::do_search(
            &registry,
            document_id.get(),
            base_revision.get(),
            &query,
            scope,
            current_sheet_index,
        )
    })
    .await
}

// ==================== Recent Files Operations ====================

#[tauri::command]
pub async fn get_recent_files(app: AppHandle) -> Result<Vec<RecentFile>, AppError> {
    blocking::run(move || recent::do_get_recent_files(&app)).await
}

#[tauri::command]
pub async fn remove_recent_file(app: AppHandle, id: String) -> Result<(), AppError> {
    blocking::run(move || recent::do_remove_recent_file(&app, &id)).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn add_recent_file_with_thumbnail(
    app: AppHandle,
    request: AddRecentFileRequest,
) -> Result<RecentFile, AppError> {
    blocking::run(move || recent::do_add_recent_file_with_thumbnail(&app, request)).await
}

#[cfg(test)]
mod tests {
    use super::{MAX_SET_CELL_CHANGES, SetCellBatch};
    use serde_json::{Value, json};

    fn cell_change(index: usize) -> Value {
        json!({
            "sheetIndex": 0,
            "row": index,
            "col": 0,
            "text": ""
        })
    }

    #[test]
    fn set_cell_batch_accepts_the_maximum_number_of_changes() {
        let changes = (0..MAX_SET_CELL_CHANGES).map(cell_change).collect();
        let batch: SetCellBatch =
            serde_json::from_value(Value::Array(changes)).expect("bounded cell batch");

        assert_eq!(batch.0.len(), MAX_SET_CELL_CHANGES);
    }

    #[test]
    fn set_cell_batch_rejects_an_oversized_sequence_during_deserialization() {
        let changes = (0..=MAX_SET_CELL_CHANGES).map(cell_change).collect();
        let error = serde_json::from_value::<SetCellBatch>(Value::Array(changes))
            .expect_err("oversized batch must be rejected");

        assert!(error.to_string().contains("at most 4096 changes"));
    }
}
