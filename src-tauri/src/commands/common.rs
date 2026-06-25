use crate::error::AppError;
#[cfg(desktop)]
use crate::io::platform::desktop;
use crate::io::{codec::writer, document};
use crate::ops::{cell_ops, editor_ops, search_ops, sort_ops};
use crate::recent::{self, AddRecentFileRequest, RecentFile};
use crate::state::{get_state, state::EditorStateInfo};
use crate::types::{
    CellValue, EditorMutationResponse, FileData, SearchResult, SearchScope, SortState,
};
use tauri::AppHandle;

// ==================== File Operations ====================

/// Desktop: 从路径直接读取并解析文件
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub fn read_file_desktop(path: String) -> Result<FileData, AppError> {
    desktop::read_file(&path)
}

/// Desktop: 生成文件字节并写入路径
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub fn save_file_desktop(path: String, file_data: FileData) -> Result<(), AppError> {
    desktop::save_file(&path, &file_data)
}

#[tauri::command]
pub fn init_file(file_data: FileData) -> Result<(), AppError> {
    document::init_file(file_data)
}

#[tauri::command(rename_all = "camelCase")]
pub fn generate_file_bytes(file_data: FileData) -> Result<Vec<u8>, AppError> {
    let (_, bytes) = writer::generate_file_bytes(&file_data)?;
    Ok(bytes)
}

#[tauri::command(rename_all = "camelCase")]
pub fn generate_thumbnail_bytes(file_data: FileData) -> Result<Vec<u8>, AppError> {
    let (_, bytes) = writer::generate_file_bytes_for_target(&file_data, "thumbnail.xlsx")?;
    Ok(bytes)
}

// ==================== Editor Operations ====================

#[tauri::command]
pub fn get_editor_state() -> Result<Option<EditorStateInfo>, AppError> {
    editor_ops::do_get_editor_state(get_state())
}

#[tauri::command]
pub fn mark_file_saved() -> Result<(), AppError> {
    editor_ops::do_mark_file_saved(get_state())
}

#[tauri::command]
pub fn undo() -> Result<EditorMutationResponse, AppError> {
    editor_ops::do_undo(get_state())
}

#[tauri::command]
pub fn redo() -> Result<EditorMutationResponse, AppError> {
    editor_ops::do_redo(get_state())
}

// ==================== Cell Operations ====================

#[tauri::command(rename_all = "camelCase")]
pub fn set_cell(
    sheet_index: usize,
    row: usize,
    col: usize,
    old_value: CellValue,
    new_value: CellValue,
) -> Result<EditorMutationResponse, AppError> {
    cell_ops::do_set_cell(get_state(), sheet_index, row, col, old_value, new_value)
}

#[tauri::command(rename_all = "camelCase")]
pub fn add_row(sheet_index: usize, row_index: usize) -> Result<EditorMutationResponse, AppError> {
    cell_ops::do_add_row(get_state(), sheet_index, row_index)
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_row(
    sheet_index: usize,
    row_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    cell_ops::do_delete_row(get_state(), sheet_index, row_index)
}

#[tauri::command(rename_all = "camelCase")]
pub fn add_column(sheet_index: usize) -> Result<EditorMutationResponse, AppError> {
    cell_ops::do_add_column(get_state(), sheet_index)
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_column(
    sheet_index: usize,
    col_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    cell_ops::do_delete_column(get_state(), sheet_index, col_index)
}

// ==================== Sheet Operations ====================

#[tauri::command]
pub fn add_sheet() -> Result<EditorMutationResponse, AppError> {
    cell_ops::do_add_sheet(get_state())
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_sheet(sheet_index: usize) -> Result<EditorMutationResponse, AppError> {
    cell_ops::do_delete_sheet(get_state(), sheet_index)
}

// ==================== Sort Operations ====================

#[tauri::command(rename_all = "camelCase")]
pub fn sort_column(
    sheet_index: usize,
    col_index: usize,
    ascending: bool,
    previous_sort_state: Option<SortState>,
) -> Result<EditorMutationResponse, AppError> {
    sort_ops::do_sort_column(
        get_state(),
        sheet_index,
        col_index,
        ascending,
        previous_sort_state,
    )
}

// ==================== Search Operations ====================

#[tauri::command(rename_all = "camelCase")]
pub fn search(
    query: String,
    scope: SearchScope,
    current_sheet_index: Option<usize>,
) -> Result<Vec<SearchResult>, AppError> {
    search_ops::do_search(get_state(), query, scope, current_sheet_index)
}

// ==================== Recent Files Operations ====================

#[tauri::command]
pub fn get_recent_files(app: AppHandle) -> Vec<RecentFile> {
    recent::do_get_recent_files(&app)
}

#[tauri::command]
pub fn remove_recent_file(app: AppHandle, id: String) -> Result<(), AppError> {
    recent::do_remove_recent_file(&app, id)
}

#[tauri::command]
pub fn check_file_exists(path: String) -> bool {
    recent::do_check_file_exists(path)
}

#[tauri::command]
pub fn get_file_size(path: String) -> i64 {
    std::fs::metadata(path)
        .map(|metadata| metadata.len() as i64)
        .unwrap_or(0)
}

#[tauri::command(rename_all = "camelCase")]
pub fn update_recent_file_path(
    app: AppHandle,
    id: String,
    new_path: String,
) -> Result<(), AppError> {
    recent::do_update_recent_file_path(&app, id, new_path)
}

#[tauri::command(rename_all = "camelCase")]
pub fn add_recent_file_with_thumbnail(
    app: AppHandle,
    request: AddRecentFileRequest,
) -> Result<RecentFile, AppError> {
    recent::do_add_recent_file_with_thumbnail(&app, request)
}
