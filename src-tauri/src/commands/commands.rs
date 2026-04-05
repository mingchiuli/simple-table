use crate::error::AppError;
use crate::ops::recent_ops::{RecentFile, StorageType};
use crate::types::{CellValue, FileData, OperationResult, SearchResult, SearchScope, SortState};
use tauri::AppHandle;

/// 全局编辑器状态（使用 Arc<RwLock> 支持多线程访问）
static EDITOR_STATE: std::sync::OnceLock<std::sync::Arc<std::sync::RwLock<Option<crate::state::editor_state::EditorState>>>> = std::sync::OnceLock::new();

pub fn get_state() -> std::sync::Arc<std::sync::RwLock<Option<crate::state::editor_state::EditorState>>> {
    EDITOR_STATE.get_or_init(|| std::sync::Arc::new(std::sync::RwLock::new(None))).clone()
}

// ==================== File Operations ====================

#[tauri::command(rename_all = "camelCase")]
pub fn read_file_bytes(path: String, bytes: Vec<u8>, file_name: Option<String>) -> Result<FileData, AppError> {
    crate::io::file_ops::do_read_file_bytes(path, bytes, file_name)
}

#[tauri::command]
pub fn generate_file_bytes(file_data: FileData) -> Result<(String, Vec<u8>), AppError> {
    crate::io::file_ops::do_generate_file_bytes(file_data)
}

#[tauri::command]
pub fn init_file(file_data: FileData) -> Result<(), AppError> {
    crate::io::file_ops::do_init_file(file_data)
}

// ==================== Editor Operations ====================

#[tauri::command]
pub fn get_editor_state() -> Result<Option<crate::state::state::EditorStateInfo>, AppError> {
    crate::ops::editor_ops::do_get_editor_state(get_state())
}

#[tauri::command]
pub fn undo() -> Result<OperationResult, AppError> {
    crate::ops::editor_ops::do_undo(get_state())
}

#[tauri::command]
pub fn redo() -> Result<OperationResult, AppError> {
    crate::ops::editor_ops::do_redo(get_state())
}

// ==================== Cell Operations ====================

#[tauri::command(rename_all = "camelCase")]
pub fn set_cell(
    sheet_index: usize,
    row: usize,
    col: usize,
    old_value: CellValue,
    new_value: CellValue,
) -> Result<(), AppError> {
    crate::ops::cell_ops::do_set_cell(get_state(), sheet_index, row, col, old_value, new_value)
}

#[tauri::command(rename_all = "camelCase")]
pub fn add_row(sheet_index: usize, row_index: usize) -> Result<(), AppError> {
    crate::ops::cell_ops::do_add_row(get_state(), sheet_index, row_index)
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_row(sheet_index: usize, row_index: usize) -> Result<(), AppError> {
    crate::ops::cell_ops::do_delete_row(get_state(), sheet_index, row_index)
}

#[tauri::command(rename_all = "camelCase")]
pub fn add_column(sheet_index: usize) -> Result<(), AppError> {
    crate::ops::cell_ops::do_add_column(get_state(), sheet_index)
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_column(sheet_index: usize, col_index: usize) -> Result<(), AppError> {
    crate::ops::cell_ops::do_delete_column(get_state(), sheet_index, col_index)
}

// ==================== Sheet Operations ====================

#[tauri::command]
pub fn add_sheet() -> Result<(), AppError> {
    crate::ops::cell_ops::do_add_sheet(get_state())
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_sheet(sheet_index: usize) -> Result<(), AppError> {
    crate::ops::cell_ops::do_delete_sheet(get_state(), sheet_index)
}

// ==================== Sort Operations ====================

#[tauri::command(rename_all = "camelCase")]
pub fn sort_column(
    sheet_index: usize,
    col_index: usize,
    ascending: bool,
    previous_sort_state: Option<SortState>,
) -> Result<OperationResult, AppError> {
    crate::ops::sort_ops::do_sort_column(get_state(), sheet_index, col_index, ascending, previous_sort_state)
}

// ==================== Search Operations ====================

#[tauri::command(rename_all = "camelCase")]
pub fn search(
    query: String,
    scope: SearchScope,
    current_sheet_index: Option<usize>,
) -> Result<Vec<SearchResult>, AppError> {
    crate::ops::search_ops::do_search(get_state(), query, scope, current_sheet_index)
}

// ==================== Recent Files Operations ====================

#[tauri::command]
pub fn get_recent_files(app: AppHandle) -> Vec<RecentFile> {
    crate::ops::recent_ops::do_get_recent_files(&app)
}

#[tauri::command]
pub fn remove_recent_file(app: AppHandle, id: String) -> Result<(), String> {
    crate::ops::recent_ops::do_remove_recent_file(&app, id)
}

#[tauri::command]
pub fn check_file_exists(path: String) -> bool {
    crate::ops::recent_ops::do_check_file_exists(path)
}

#[tauri::command]
pub fn update_recent_file_path(app: AppHandle, id: String, new_path: String) -> Result<(), String> {
    crate::ops::recent_ops::do_update_recent_file_path(&app, id, new_path)
}

#[tauri::command(rename_all = "camelCase")]
pub fn add_recent_file_with_thumbnail(
    app: AppHandle,
    path: String,
    file_name: String,
    file_size: i64,
    bytes: Vec<u8>,
    extension: String,
    storage_type: Option<String>,
    original_path: Option<String>,
) -> Result<RecentFile, String> {
    // 将字符串转换为 StorageType 枚举
    let st = storage_type.and_then(|s| match s.as_str() {
        "androidUri" => Some(StorageType::AndroidUri),
        "iosPrivate" => Some(StorageType::IosPrivate),
        "desktopPath" => Some(StorageType::DesktopPath),
        _ => None,
    });

    crate::ops::recent_ops::do_add_recent_file_with_thumbnail(
        &app,
        path,
        file_name,
        file_size,
        bytes,
        extension,
        st,
        original_path,
    )
}
