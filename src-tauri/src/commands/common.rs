use crate::error::AppError;
use crate::io::document;
#[cfg(desktop)]
use crate::io::platform::desktop;
use crate::ops::{cell_ops, editor_ops, search_ops};
use crate::recent::{self, AddRecentFileRequest, RecentFile};
use crate::state::{get_registry, state::EditorSessionInfo};
use crate::types::{
    DocumentCapabilities, EditorMutationResponse, FileData, SearchResult, SearchScope,
    SetCellRequest,
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
pub fn save_file_desktop(path: String) -> Result<(), AppError> {
    desktop::save_file(&path)
}

#[tauri::command]
pub fn init_file(file_data: FileData) -> Result<(), AppError> {
    document::init_file(file_data)
}

#[tauri::command]
pub fn generate_current_thumbnail_bytes() -> Result<Vec<u8>, AppError> {
    let (_, bytes) = document::generate_current_file_bytes_for_target("thumbnail.xlsx")?;
    Ok(bytes)
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_document_capabilities(
    file_name: String,
    current_path: Option<String>,
) -> DocumentCapabilities {
    document::document_capabilities(file_name, current_path)
}

// ==================== Editor Operations ====================

#[tauri::command]
pub fn get_editor_state() -> Result<Option<EditorSessionInfo>, AppError> {
    editor_ops::do_get_editor_state(get_registry())
}

#[tauri::command]
pub fn mark_file_saved() -> Result<(), AppError> {
    editor_ops::do_mark_file_saved(get_registry())
}

#[tauri::command]
pub fn undo() -> Result<EditorMutationResponse, AppError> {
    editor_ops::do_undo(get_registry())
}

#[tauri::command]
pub fn redo() -> Result<EditorMutationResponse, AppError> {
    editor_ops::do_redo(get_registry())
}

// ==================== Cell Operations ====================

#[tauri::command(rename_all = "camelCase")]
pub fn set_cell(
    sheet_index: usize,
    row: usize,
    col: usize,
    text: String,
) -> Result<EditorMutationResponse, AppError> {
    cell_ops::do_set_cell(get_registry(), sheet_index, row, col, text)
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_cells(changes: Vec<SetCellRequest>) -> Result<EditorMutationResponse, AppError> {
    cell_ops::do_set_cells(get_registry(), changes)
}

#[tauri::command(rename_all = "camelCase")]
pub fn add_row(sheet_index: usize, row_index: usize) -> Result<EditorMutationResponse, AppError> {
    cell_ops::do_add_row(get_registry(), sheet_index, row_index)
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_row(
    sheet_index: usize,
    row_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    cell_ops::do_delete_row(get_registry(), sheet_index, row_index)
}

#[tauri::command(rename_all = "camelCase")]
pub fn add_column(sheet_index: usize) -> Result<EditorMutationResponse, AppError> {
    cell_ops::do_add_column(get_registry(), sheet_index)
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_column(
    sheet_index: usize,
    col_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    cell_ops::do_delete_column(get_registry(), sheet_index, col_index)
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_column_width(
    sheet_index: usize,
    col_index: usize,
    width: Option<u32>,
) -> Result<EditorMutationResponse, AppError> {
    cell_ops::do_set_column_width(get_registry(), sheet_index, col_index, width)
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_row_height(
    sheet_index: usize,
    row_index: usize,
    height: Option<u32>,
) -> Result<EditorMutationResponse, AppError> {
    cell_ops::do_set_row_height(get_registry(), sheet_index, row_index, height)
}

// ==================== Sheet Operations ====================

#[tauri::command]
pub fn add_sheet() -> Result<EditorMutationResponse, AppError> {
    cell_ops::do_add_sheet(get_registry())
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_sheet(sheet_index: usize) -> Result<EditorMutationResponse, AppError> {
    cell_ops::do_delete_sheet(get_registry(), sheet_index)
}

// ==================== Search Operations ====================

#[tauri::command(rename_all = "camelCase")]
pub fn search(
    query: String,
    scope: SearchScope,
    current_sheet_index: Option<usize>,
) -> Result<Vec<SearchResult>, AppError> {
    search_ops::do_search(get_registry(), query, scope, current_sheet_index)
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
