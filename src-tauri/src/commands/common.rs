#![allow(clippy::needless_pass_by_value)]

use crate::error::AppError;
use crate::io::document;
#[cfg(desktop)]
use crate::io::platform::desktop;
use crate::ops::{cell_ops, editor_ops, search_ops};
use crate::recent::{self, AddRecentFileRequest, RecentFile};
use crate::state::{active_document_store, state::EditorSessionInfo};
use crate::types::{
    DocumentCapabilities, EditorMutationResponse, FileData, NativeSavePlan, OpenDocumentResponse,
    SavedDocumentResponse, SearchResult, SearchScope, SetCellRequest, SpreadsheetFormatOptions,
};
use tauri::AppHandle;

// ==================== File Operations ====================

/// Desktop: 从路径直接读取并解析文件
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub fn read_file_desktop(path: String) -> Result<OpenDocumentResponse, AppError> {
    desktop::read_file(&path)
}

/// Desktop: 生成文件字节并写入路径
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub fn save_file_desktop(path: String) -> Result<SavedDocumentResponse, AppError> {
    desktop::save_file(&path)
}

/// Desktop: 导出当前内容到指定路径，不改变当前编辑文档身份。
#[cfg(desktop)]
#[tauri::command(rename_all = "camelCase")]
pub fn export_file_desktop(path: String) -> Result<(), AppError> {
    desktop::export_file(&path)
}

#[tauri::command]
pub fn init_file(file_data: FileData) -> Result<OpenDocumentResponse, AppError> {
    document::init_file(file_data)
}

#[tauri::command]
pub fn get_current_file_data() -> Result<FileData, AppError> {
    document::current_file_data()
}

#[tauri::command]
pub fn close_current_document() -> Result<(), AppError> {
    document::close_current_document()
}

#[tauri::command(rename_all = "camelCase")]
pub fn update_document_identity(path: String, file_name: String) -> Result<(), AppError> {
    document::update_current_file_identity(path, file_name)
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_document_capabilities(
    file_name: String,
    current_path: Option<String>,
) -> DocumentCapabilities {
    document::document_capabilities(&file_name, current_path.as_deref())
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_native_save_plan(target_path_or_name: String) -> NativeSavePlan {
    document::native_save_plan(&target_path_or_name)
}

#[tauri::command]
pub fn get_spreadsheet_format_options() -> SpreadsheetFormatOptions {
    document::format_options()
}

// ==================== Editor Operations ====================

#[tauri::command]
pub fn get_editor_state() -> Result<Option<EditorSessionInfo>, AppError> {
    let registry = active_document_store();
    editor_ops::do_get_editor_state(&registry)
}

#[tauri::command(rename_all = "camelCase")]
pub fn undo(document_id: u64, base_revision: u64) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    editor_ops::do_undo(&registry, document_id, base_revision)
}

#[tauri::command(rename_all = "camelCase")]
pub fn redo(document_id: u64, base_revision: u64) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    editor_ops::do_redo(&registry, document_id, base_revision)
}

// ==================== Cell Operations ====================

#[tauri::command(rename_all = "camelCase")]
pub fn set_cell(
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    row: usize,
    col: usize,
    text: String,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    cell_ops::do_set_cell(
        &registry,
        document_id,
        base_revision,
        sheet_index,
        row,
        col,
        text,
    )
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_cells(
    document_id: u64,
    base_revision: u64,
    changes: Vec<SetCellRequest>,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    cell_ops::do_set_cells(&registry, document_id, base_revision, changes)
}

#[tauri::command(rename_all = "camelCase")]
pub fn add_row(
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    row_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    cell_ops::do_add_row(
        &registry,
        document_id,
        base_revision,
        sheet_index,
        row_index,
    )
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_row(
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    row_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    cell_ops::do_delete_row(
        &registry,
        document_id,
        base_revision,
        sheet_index,
        row_index,
    )
}

#[tauri::command(rename_all = "camelCase")]
pub fn add_column(
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    col_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    cell_ops::do_add_column(
        &registry,
        document_id,
        base_revision,
        sheet_index,
        col_index,
    )
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_column(
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    col_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    cell_ops::do_delete_column(
        &registry,
        document_id,
        base_revision,
        sheet_index,
        col_index,
    )
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_column_width(
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    col_index: usize,
    width: Option<u32>,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    cell_ops::do_set_column_width(
        &registry,
        document_id,
        base_revision,
        sheet_index,
        col_index,
        width,
    )
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_row_height(
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    row_index: usize,
    height: Option<u32>,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    cell_ops::do_set_row_height(
        &registry,
        document_id,
        base_revision,
        sheet_index,
        row_index,
        height,
    )
}

// ==================== Sheet Operations ====================

#[tauri::command(rename_all = "camelCase")]
pub fn add_sheet(document_id: u64, base_revision: u64) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    cell_ops::do_add_sheet(&registry, document_id, base_revision)
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_sheet(
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    cell_ops::do_delete_sheet(&registry, document_id, base_revision, sheet_index)
}

// ==================== Search Operations ====================

#[tauri::command(rename_all = "camelCase")]
pub fn search(
    query: String,
    scope: SearchScope,
    current_sheet_index: Option<usize>,
) -> Result<Vec<SearchResult>, AppError> {
    let registry = active_document_store();
    search_ops::do_search(&registry, &query, scope, current_sheet_index)
}

// ==================== Recent Files Operations ====================

#[tauri::command]
pub fn get_recent_files(app: AppHandle) -> Vec<RecentFile> {
    recent::do_get_recent_files(&app)
}

#[tauri::command]
pub fn remove_recent_file(app: AppHandle, id: String) -> Result<(), AppError> {
    recent::do_remove_recent_file(&app, &id)
}

#[tauri::command]
pub fn check_file_exists(path: String) -> bool {
    recent::do_check_file_exists(&path)
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
    recent::do_update_recent_file_path(&app, &id, &new_path)
}

#[tauri::command(rename_all = "camelCase")]
pub fn add_recent_file_with_thumbnail(
    app: AppHandle,
    request: AddRecentFileRequest,
) -> Result<RecentFile, AppError> {
    recent::do_add_recent_file_with_thumbnail(&app, request)
}
