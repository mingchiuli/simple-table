#![allow(clippy::needless_pass_by_value)]

use super::{
    CommandU64, blocking, mutation_executor, projection_executor, recent_executor, search_executor,
};
use crate::application::editor_command_service::EditorSessionInfo;
use crate::application::runtime::ApplicationRuntime;
use crate::application::{
    document_open_service, document_query_service, document_save_service, document_service,
    editor_command_service,
};
use crate::error::AppError;
#[cfg(desktop)]
use crate::io::platform::desktop;
use crate::recent::{self, AddRecentFileRequest, RecentFile};
use crate::resource_limits::{MAX_CELL_TEXT_BYTES, MAX_MUTATION_TEXT_BYTES};
#[cfg(desktop)]
use crate::types::SavedDocumentResponse;
use crate::types::{
    DocumentCapabilities, EditorMutationResponse, MutationResultLookup, NativeSavePlan,
    OpenDocumentResponse, PreparedOpenDocument, SearchResponse, SearchScope, SetCellRequest,
    SheetRegion, SheetRegionProjectionResponse, SpreadsheetFormatOptions,
};
use tauri::{AppHandle, State};

const MAX_SET_CELL_CHANGES: usize = 4_096;

#[derive(Debug)]
pub(crate) struct BoundedCellText(String);

impl BoundedCellText {
    fn into_inner(self) -> String {
        self.0
    }
}

impl<'de> serde::Deserialize<'de> for BoundedCellText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct CellTextVisitor;

        impl serde::de::Visitor<'_> for CellTextVisitor {
            type Value = BoundedCellText;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(
                    formatter,
                    "cell text containing at most {MAX_CELL_TEXT_BYTES} bytes"
                )
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                validate_cell_text_bytes(value.len()).map(|()| BoundedCellText(value.to_string()))
            }

            fn visit_borrowed_str<E>(self, value: &'_ str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(value)
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                validate_cell_text_bytes(value.len()).map(|()| BoundedCellText(value))
            }
        }

        fn validate_cell_text_bytes<E>(byte_count: usize) -> Result<(), E>
        where
            E: serde::de::Error,
        {
            if byte_count <= MAX_CELL_TEXT_BYTES {
                return Ok(());
            }
            Err(E::custom(format!(
                "cell text requires {byte_count} bytes; the maximum is {MAX_CELL_TEXT_BYTES} bytes"
            )))
        }

        deserializer.deserialize_string(CellTextVisitor)
    }
}

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

        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct BoundedSetCellRequest {
            sheet_index: usize,
            row: usize,
            col: usize,
            text: BoundedCellText,
        }

        impl From<BoundedSetCellRequest> for SetCellRequest {
            fn from(request: BoundedSetCellRequest) -> Self {
                Self {
                    sheet_index: request.sheet_index,
                    row: request.row,
                    col: request.col,
                    text: request.text.into_inner(),
                }
            }
        }

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
                let mut text_bytes = 0usize;
                while changes.len() < MAX_SET_CELL_CHANGES {
                    let Some(change) = sequence.next_element::<BoundedSetCellRequest>()? else {
                        return Ok(SetCellBatch(changes));
                    };
                    text_bytes = text_bytes.checked_add(change.text.0.len()).ok_or_else(|| {
                        serde::de::Error::custom("set_cells text bytes overflowed")
                    })?;
                    if text_bytes > MAX_MUTATION_TEXT_BYTES {
                        return Err(serde::de::Error::custom(format!(
                            "set_cells text requires {text_bytes} bytes; the maximum is {MAX_MUTATION_TEXT_BYTES} bytes"
                        )));
                    }
                    changes.push(change.into());
                }
                if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                    return Err(serde::de::Error::custom(format!(
                        "set_cells accepts at most {MAX_SET_CELL_CHANGES} changes"
                    )));
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
pub async fn prepare_open_file_desktop(
    runtime: State<'_, ApplicationRuntime>,
    path: String,
) -> Result<PreparedOpenDocument, AppError> {
    let runtime = runtime.inner().clone();
    blocking::run(move || document_open_service::prepare_open_file_desktop(&runtime, &path)).await
}

/// Desktop: 通过最近文件 id 读取后端 recent store 中的路径。
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub async fn prepare_recent_file_desktop(
    runtime: State<'_, ApplicationRuntime>,
    app: AppHandle,
    id: String,
) -> Result<PreparedOpenDocument, AppError> {
    let runtime = runtime.inner().clone();
    blocking::run(move || document_open_service::prepare_recent_file_desktop(&runtime, &app, &id))
        .await
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
    runtime: State<'_, ApplicationRuntime>,
    path: String,
    document_id: CommandU64,
    base_revision: CommandU64,
) -> Result<SavedDocumentResponse, AppError> {
    let runtime = runtime.inner().clone();
    blocking::run(move || {
        document_save_service::save_file_desktop(
            &runtime,
            &path,
            document_id.get(),
            base_revision.get(),
        )
    })
    .await
}

/// Desktop: 导出当前内容到指定路径，不改变当前编辑文档身份。
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub async fn export_file_desktop(
    runtime: State<'_, ApplicationRuntime>,
    app: AppHandle,
    default_name: String,
    document_id: CommandU64,
    base_revision: CommandU64,
) -> Result<Option<String>, AppError> {
    let runtime = runtime.inner().clone();
    blocking::run(move || {
        document_save_service::export_file_desktop(
            &runtime,
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
) -> Result<PreparedOpenDocument, AppError> {
    let runtime = runtime.inner().clone();
    blocking::run(move || document_open_service::prepare_new_file(&runtime)).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn commit_prepared_document(
    runtime: State<'_, ApplicationRuntime>,
    token: String,
    expected_document_id: Option<CommandU64>,
    expected_revision: Option<CommandU64>,
) -> Result<OpenDocumentResponse, AppError> {
    let runtime = runtime.inner().clone();
    mutation_executor::run(move || {
        document_service::commit_prepared_document(
            &runtime,
            &token,
            expected_document_id.map(CommandU64::get),
            expected_revision.map(CommandU64::get),
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn abort_prepared_document(
    runtime: State<'_, ApplicationRuntime>,
    token: String,
) -> Result<(), AppError> {
    let runtime = runtime.inner().clone();
    blocking::run(move || document_open_service::abort_prepared_document(&runtime, &token)).await
}

#[tauri::command]
pub async fn get_active_document(
    runtime: State<'_, ApplicationRuntime>,
) -> Result<Option<OpenDocumentResponse>, AppError> {
    let runtime = runtime.inner().clone();
    projection_executor::run(move || document_query_service::active_document_response(&runtime))
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_mutation_result(
    runtime: State<'_, ApplicationRuntime>,
    document_id: CommandU64,
    command_id: String,
) -> Result<MutationResultLookup, AppError> {
    editor_command_service::get_mutation_result(&runtime, document_id.get(), &command_id)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_current_document_projection(
    runtime: State<'_, ApplicationRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    preferred_sheet_index: usize,
) -> Result<OpenDocumentResponse, AppError> {
    let runtime = runtime.inner().clone();
    projection_executor::run(move || {
        document_query_service::current_document_projection_for_command(
            &runtime,
            document_id.get(),
            base_revision.get(),
            preferred_sheet_index,
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_sheet_region_projection(
    runtime: State<'_, ApplicationRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    region: SheetRegion,
) -> Result<SheetRegionProjectionResponse, AppError> {
    let runtime = runtime.inner().clone();
    projection_executor::run(move || {
        document_query_service::sheet_region_projection_for_command(
            &runtime,
            document_id.get(),
            base_revision.get(),
            region,
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn close_current_document(
    runtime: State<'_, ApplicationRuntime>,
    document_id: CommandU64,
) -> Result<(), AppError> {
    let runtime = runtime.inner().clone();
    mutation_executor::run(move || {
        document_service::close_current_document(&runtime, document_id.get())
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
        &runtime,
        document_id.get(),
        base_revision.get(),
    )
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_native_save_plan(
    runtime: State<'_, ApplicationRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    target_path_or_name: String,
) -> Result<NativeSavePlan, AppError> {
    document_query_service::native_save_plan_for_command(
        &runtime,
        document_id.get(),
        base_revision.get(),
        &target_path_or_name,
    )
}

#[tauri::command]
pub fn get_spreadsheet_format_options() -> SpreadsheetFormatOptions {
    document_query_service::format_options()
}

// ==================== Editor Operations ====================

#[tauri::command(rename_all = "camelCase")]
pub fn get_editor_state(
    runtime: State<'_, ApplicationRuntime>,
    document_id: Option<CommandU64>,
    base_revision: Option<CommandU64>,
) -> Result<Option<EditorSessionInfo>, AppError> {
    editor_command_service::get_editor_state(
        &runtime,
        document_id.map(CommandU64::get),
        base_revision.map(CommandU64::get),
    )
}

#[tauri::command(rename_all = "camelCase")]
pub async fn undo(
    runtime: State<'_, ApplicationRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    mutation_executor::run(move || {
        editor_command_service::undo(
            &runtime,
            document_id.get(),
            base_revision.get(),
            &command_id,
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn redo(
    runtime: State<'_, ApplicationRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    mutation_executor::run(move || {
        editor_command_service::redo(
            &runtime,
            document_id.get(),
            base_revision.get(),
            &command_id,
        )
    })
    .await
}

// ==================== Cell Operations ====================

#[tauri::command(rename_all = "camelCase")]
pub async fn set_cell(
    runtime: State<'_, ApplicationRuntime>,
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
    mutation_executor::run(move || {
        editor_command_service::set_cell(
            &runtime,
            document_id.get(),
            base_revision.get(),
            &command_id,
            sheet_index,
            row,
            col,
            text,
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_cells(
    runtime: State<'_, ApplicationRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    changes: SetCellBatch,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    let changes = changes.into_inner();
    mutation_executor::run(move || {
        editor_command_service::set_cells(
            &runtime,
            document_id.get(),
            base_revision.get(),
            &command_id,
            changes,
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn add_row(
    runtime: State<'_, ApplicationRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    row_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    mutation_executor::run(move || {
        editor_command_service::add_row(
            &runtime,
            document_id.get(),
            base_revision.get(),
            &command_id,
            sheet_index,
            row_index,
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_row(
    runtime: State<'_, ApplicationRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    row_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    mutation_executor::run(move || {
        editor_command_service::delete_row(
            &runtime,
            document_id.get(),
            base_revision.get(),
            &command_id,
            sheet_index,
            row_index,
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn add_column(
    runtime: State<'_, ApplicationRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    col_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    mutation_executor::run(move || {
        editor_command_service::add_column(
            &runtime,
            document_id.get(),
            base_revision.get(),
            &command_id,
            sheet_index,
            col_index,
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_column(
    runtime: State<'_, ApplicationRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    col_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    mutation_executor::run(move || {
        editor_command_service::delete_column(
            &runtime,
            document_id.get(),
            base_revision.get(),
            &command_id,
            sheet_index,
            col_index,
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_column_width(
    runtime: State<'_, ApplicationRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    col_index: usize,
    width: Option<u32>,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    mutation_executor::run(move || {
        editor_command_service::set_column_width(
            &runtime,
            document_id.get(),
            base_revision.get(),
            &command_id,
            sheet_index,
            col_index,
            width,
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_row_height(
    runtime: State<'_, ApplicationRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
    row_index: usize,
    height: Option<u32>,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    mutation_executor::run(move || {
        editor_command_service::set_row_height(
            &runtime,
            document_id.get(),
            base_revision.get(),
            &command_id,
            sheet_index,
            row_index,
            height,
        )
    })
    .await
}

// ==================== Sheet Operations ====================

#[tauri::command(rename_all = "camelCase")]
pub async fn add_sheet(
    runtime: State<'_, ApplicationRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    mutation_executor::run(move || {
        editor_command_service::add_sheet(
            &runtime,
            document_id.get(),
            base_revision.get(),
            &command_id,
        )
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_sheet(
    runtime: State<'_, ApplicationRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    command_id: String,
    sheet_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let runtime = runtime.inner().clone();
    mutation_executor::run(move || {
        editor_command_service::delete_sheet(
            &runtime,
            document_id.get(),
            base_revision.get(),
            &command_id,
            sheet_index,
        )
    })
    .await
}

// ==================== Search Operations ====================

#[tauri::command(rename_all = "camelCase")]
pub async fn search(
    runtime: State<'_, ApplicationRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    query: String,
    scope: SearchScope,
    current_sheet_index: Option<usize>,
) -> Result<SearchResponse, AppError> {
    let runtime = runtime.inner().clone();
    search_executor::run(move || {
        editor_command_service::search(
            &runtime,
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
    recent_executor::run(move || recent::do_get_recent_files(&app)).await
}

#[tauri::command]
pub async fn remove_recent_file(
    runtime: State<'_, ApplicationRuntime>,
    app: AppHandle,
    id: String,
) -> Result<(), AppError> {
    let runtime = runtime.inner().clone();
    recent_executor::run(move || recent::do_remove_recent_file(&runtime, &app, &id)).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn add_recent_file_with_thumbnail(
    runtime: State<'_, ApplicationRuntime>,
    app: AppHandle,
    request: AddRecentFileRequest,
) -> Result<RecentFile, AppError> {
    let runtime = runtime.inner().clone();
    recent_executor::run(move || recent::do_add_recent_file_with_thumbnail(&runtime, &app, request))
        .await
}

#[cfg(test)]
mod tests {
    use super::{
        BoundedCellText, MAX_CELL_TEXT_BYTES, MAX_MUTATION_TEXT_BYTES, MAX_SET_CELL_CHANGES,
        SetCellBatch,
    };
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

    #[test]
    fn set_cell_batch_rejects_a_single_oversized_cell_during_deserialization() {
        let changes = vec![json!({
            "sheetIndex": 0,
            "row": 0,
            "col": 0,
            "text": "x".repeat(MAX_CELL_TEXT_BYTES + 1)
        })];
        let error = serde_json::from_value::<SetCellBatch>(Value::Array(changes))
            .expect_err("oversized cell text must be rejected");

        assert!(error.to_string().contains("cell text requires"));
    }

    #[test]
    fn single_cell_text_uses_the_same_deserialization_limit() {
        let accepted = serde_json::from_value::<BoundedCellText>(Value::String(
            "x".repeat(MAX_CELL_TEXT_BYTES),
        ));
        assert!(accepted.is_ok());

        let error = serde_json::from_value::<BoundedCellText>(Value::String(
            "x".repeat(MAX_CELL_TEXT_BYTES + 1),
        ))
        .expect_err("oversized single cell text");
        assert!(error.to_string().contains("cell text requires"));
    }

    #[test]
    fn set_cell_batch_rejects_aggregate_text_during_deserialization() {
        let changes = vec![
            json!({ "sheetIndex": 0, "row": 0, "col": 0, "text": "x".repeat(MAX_CELL_TEXT_BYTES) }),
            json!({ "sheetIndex": 0, "row": 1, "col": 0, "text": "x".repeat(MAX_CELL_TEXT_BYTES) }),
            json!({ "sheetIndex": 0, "row": 2, "col": 0, "text": "x" }),
        ];
        let error = serde_json::from_value::<SetCellBatch>(Value::Array(changes))
            .expect_err("oversized aggregate text must be rejected");

        assert!(
            error
                .to_string()
                .contains(&format!("maximum is {MAX_MUTATION_TEXT_BYTES} bytes"))
        );
    }
}
