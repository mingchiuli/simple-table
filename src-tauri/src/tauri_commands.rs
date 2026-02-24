use crate::error::AppError;
use crate::types::{CellValue, FileData, OperationResult, SearchResult, SearchScope};

/// 全局编辑器状态（使用 Arc<RwLock> 支持多线程访问）
static EDITOR_STATE: std::sync::OnceLock<std::sync::Arc<std::sync::RwLock<Option<crate::command::EditorState>>>> = std::sync::OnceLock::new();

pub fn get_state() -> std::sync::Arc<std::sync::RwLock<Option<crate::command::EditorState>>> {
    EDITOR_STATE.get_or_init(|| std::sync::Arc::new(std::sync::RwLock::new(None))).clone()
}

// ==================== File Operations ====================

/// 读取文件
#[tauri::command]
pub fn read_file(path: String) -> Result<FileData, AppError> {
    crate::file_ops::do_read_file(path)
}

/// 保存文件
#[tauri::command]
pub fn save_file(path: String, file_data: FileData) -> Result<(), AppError> {
    crate::file_ops::do_save_file(path, file_data)
}

/// 获取默认保存路径
#[tauri::command]
pub fn get_default_save_path(file_name: String) -> String {
    crate::file_ops::do_get_default_save_path(file_name)
}

/// 初始化文件（用于新建文件时初始化编辑器状态）
#[tauri::command]
pub fn init_file(file_data: FileData) -> Result<(), AppError> {
    crate::file_ops::do_init_file(file_data)
}

// ==================== Editor Operations ====================

/// 获取当前文件数据
#[tauri::command]
pub fn get_file_data() -> Result<FileData, AppError> {
    let state = get_state();
    let guard = state.read().unwrap();
    match guard.as_ref() {
        Some(editor_state) => Ok(editor_state.file_data.clone()),
        None => Err(AppError::Internal("No file loaded".to_string())),
    }
}

/// 获取编辑器状态（包含能否撤销/重做）
#[tauri::command]
pub fn get_editor_state() -> Result<Option<crate::state::EditorStateInfo>, AppError> {
    crate::editor_ops::do_get_editor_state(get_state())
}

/// 撤销操作
#[tauri::command]
pub fn undo() -> Result<OperationResult, AppError> {
    crate::editor_ops::do_undo(get_state())
}

/// 重做操作
#[tauri::command]
pub fn redo() -> Result<OperationResult, AppError> {
    crate::editor_ops::do_redo(get_state())
}

// ==================== Cell Operations ====================

/// 设置单元格值
#[tauri::command]
pub fn set_cell(
    sheet_index: usize,
    row: usize,
    col: usize,
    old_value: CellValue,
    new_value: CellValue,
) -> Result<OperationResult, AppError> {
    crate::cell_ops::do_set_cell(get_state(), sheet_index, row, col, old_value, new_value)
}

/// 添加行
#[tauri::command]
pub fn add_row(sheet_index: usize, row_index: usize) -> Result<OperationResult, AppError> {
    crate::cell_ops::do_add_row(get_state(), sheet_index, row_index)
}

/// 删除行
#[tauri::command]
pub fn delete_row(sheet_index: usize, row_index: usize, row_data: Vec<CellValue>) -> Result<OperationResult, AppError> {
    crate::cell_ops::do_delete_row(get_state(), sheet_index, row_index, row_data)
}

/// 添加列
#[tauri::command]
pub fn add_column(sheet_index: usize) -> Result<OperationResult, AppError> {
    crate::cell_ops::do_add_column(get_state(), sheet_index)
}

/// 删除列
#[tauri::command]
pub fn delete_column(sheet_index: usize, col_index: usize, col_data: Vec<CellValue>) -> Result<OperationResult, AppError> {
    crate::cell_ops::do_delete_column(get_state(), sheet_index, col_index, col_data)
}

// ==================== Sheet Operations ====================

/// 添加 Sheet
#[tauri::command]
pub fn add_sheet() -> Result<OperationResult, AppError> {
    crate::cell_ops::do_add_sheet(get_state())
}

/// 删除 Sheet
#[tauri::command]
pub fn delete_sheet(sheet_index: usize) -> Result<OperationResult, AppError> {
    crate::cell_ops::do_delete_sheet(get_state(), sheet_index)
}

// ==================== Search Operations ====================

/// 搜索单元格
#[tauri::command]
pub fn search(
    query: String,
    scope: SearchScope,
    current_sheet_index: Option<usize>,
) -> Result<Vec<SearchResult>, AppError> {
    crate::search_ops::do_search(get_state(), query, scope, current_sheet_index)
}
