use std::path::Path;
use std::sync::{Mutex, OnceLock};

use crate::command::{EditorState, Operation};
use crate::error::AppError;
use crate::reader;
use crate::types::{CellValue, FileData, OperationResult};
use crate::writer;

/// 全局编辑器状态
static EDITOR_STATE: OnceLock<Mutex<Option<EditorState>>> = OnceLock::new();

fn get_state() -> &'static Mutex<Option<EditorState>> {
    EDITOR_STATE.get_or_init(|| Mutex::new(None))
}

#[tauri::command]
pub fn read_file(path: String) -> Result<FileData, AppError> {
    let path = Path::new(&path);
    let file_data = reader::read_file(path)?;

    // 初始化编辑器状态
    let mut state = get_state().lock().unwrap();
    *state = Some(EditorState::new(file_data.clone()));

    Ok(file_data)
}

#[tauri::command]
pub fn save_file(path: String, file_data: FileData) -> Result<(), AppError> {
    let path = Path::new(&path);
    writer::save_file(path, &file_data)?;

    // 更新编辑器状态中的文件数据
    let mut state = get_state().lock().unwrap();
    if let Some(editor_state) = state.as_mut() {
        editor_state.file_data = file_data;
    }

    Ok(())
}

#[tauri::command]
pub fn get_default_save_path(file_name: String) -> String {
    if let Some(dot_pos) = file_name.rfind('.') {
        let name = &file_name[..dot_pos];
        format!("{}_edited.xlsx", name)
    } else {
        format!("{}_edited.xlsx", file_name)
    }
}

/// 获取编辑器状态（包含能否撤销/重做）
#[derive(serde::Serialize)]
pub struct EditorStateInfo {
    pub can_undo: bool,
    pub can_redo: bool,
}

#[tauri::command]
pub fn get_editor_state() -> Result<Option<EditorStateInfo>, AppError> {
    let state = get_state().lock().unwrap();
    Ok(state.as_ref().map(|s| EditorStateInfo {
        can_undo: s.can_undo,
        can_redo: s.can_redo,
    }))
}

/// 撤销操作
#[tauri::command]
pub fn undo() -> Result<OperationResult, AppError> {
    let mut state = get_state().lock().unwrap();
    match state.as_mut() {
        Some(editor_state) => {
            if let Some(result) = editor_state.undo() {
                Ok(result)
            } else {
                Err(AppError::Internal("Nothing to undo".to_string()))
            }
        }
        None => Err(AppError::Internal("No file loaded".to_string())),
    }
}

/// 重做操作
#[tauri::command]
pub fn redo() -> Result<OperationResult, AppError> {
    let mut state = get_state().lock().unwrap();
    match state.as_mut() {
        Some(editor_state) => {
            if let Some(result) = editor_state.redo() {
                Ok(result)
            } else {
                Err(AppError::Internal("Nothing to redo".to_string()))
            }
        }
        None => Err(AppError::Internal("No file loaded".to_string())),
    }
}

/// 设置单元格值
#[tauri::command]
pub fn set_cell(
    sheet_index: usize,
    row: usize,
    col: usize,
    old_value: CellValue,
    new_value: CellValue,
) -> Result<OperationResult, AppError> {
    let mut state = get_state().lock().unwrap();
    match state.as_mut() {
        Some(editor_state) => {
            let operation = Operation::SetCell {
                sheet_index,
                row,
                col,
                old_value,
                new_value,
            };
            Ok(editor_state.execute(operation))
        }
        None => Err(AppError::Internal("No file loaded".to_string())),
    }
}

/// 添加行
#[tauri::command]
pub fn add_row(sheet_index: usize, row_index: usize) -> Result<OperationResult, AppError> {
    let mut state = get_state().lock().unwrap();
    match state.as_mut() {
        Some(editor_state) => {
            let operation = Operation::AddRow {
                sheet_index,
                row_index,
            };
            Ok(editor_state.execute(operation))
        }
        None => Err(AppError::Internal("No file loaded".to_string())),
    }
}

/// 删除行
#[tauri::command]
pub fn delete_row(sheet_index: usize, row_index: usize, row_data: Vec<CellValue>) -> Result<OperationResult, AppError> {
    let mut state = get_state().lock().unwrap();
    match state.as_mut() {
        Some(editor_state) => {
            let operation = Operation::DeleteRow {
                sheet_index,
                row_index,
                row_data,
            };
            Ok(editor_state.execute(operation))
        }
        None => Err(AppError::Internal("No file loaded".to_string())),
    }
}

/// 添加列
#[tauri::command]
pub fn add_column(sheet_index: usize) -> Result<OperationResult, AppError> {
    let mut state = get_state().lock().unwrap();
    match state.as_mut() {
        Some(editor_state) => {
            let operation = Operation::AddColumn { sheet_index };
            Ok(editor_state.execute(operation))
        }
        None => Err(AppError::Internal("No file loaded".to_string())),
    }
}

/// 删除列
#[tauri::command]
pub fn delete_column(sheet_index: usize, col_index: usize) -> Result<OperationResult, AppError> {
    let mut state = get_state().lock().unwrap();
    match state.as_mut() {
        Some(editor_state) => {
            let operation = Operation::DeleteColumn {
                sheet_index,
                col_index,
            };
            Ok(editor_state.execute(operation))
        }
        None => Err(AppError::Internal("No file loaded".to_string())),
    }
}
