use std::sync::Arc;
use std::sync::RwLock;

use crate::command::EditorState;
use crate::error::AppError;
use crate::types::{CellValue, OperationResult};

/// 异步重建指定 sheet 的索引
fn spawn_rebuild_sheet_index(sheet_index: usize, state: Arc<RwLock<Option<EditorState>>>) {
    std::thread::spawn(move || {
        if let Ok(mut guard) = state.write() {
            if let Some(ref mut editor_state) = *guard {
                if let Some(sheet) = editor_state.file_data.sheets.get_mut(sheet_index) {
                    crate::command::rebuild_sheet_index(sheet);
                }
            }
        }
    });
}

/// 设置单元格值
pub fn do_set_cell(
    state: Arc<RwLock<Option<EditorState>>>,
    sheet_index: usize,
    row: usize,
    col: usize,
    old_value: CellValue,
    new_value: CellValue,
) -> Result<OperationResult, AppError> {
    let mut state = state.write().unwrap();
    match state.as_mut() {
        Some(editor_state) => {
            let operation = crate::command::Operation::SetCell {
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
pub fn do_add_row(state: Arc<RwLock<Option<EditorState>>>, sheet_index: usize, row_index: usize) -> Result<OperationResult, AppError> {
    let result = {
        let mut state_guard = state.write().unwrap();
        match state_guard.as_mut() {
            Some(editor_state) => {
                let operation = crate::command::Operation::AddRow {
                    sheet_index,
                    row_index,
                };
                Ok(editor_state.execute(operation))
            }
            None => Err(AppError::Internal("No file loaded".to_string())),
        }
    };

    // 异步重建索引
    if result.is_ok() {
        spawn_rebuild_sheet_index(sheet_index, state.clone());
    }

    result
}

/// 删除行
pub fn do_delete_row(state: Arc<RwLock<Option<EditorState>>>, sheet_index: usize, row_index: usize, row_data: Vec<CellValue>) -> Result<OperationResult, AppError> {
    let result = {
        let mut state_guard = state.write().unwrap();
        match state_guard.as_mut() {
            Some(editor_state) => {
                let operation = crate::command::Operation::DeleteRow {
                    sheet_index,
                    row_index,
                    row_data,
                };
                Ok(editor_state.execute(operation))
            }
            None => Err(AppError::Internal("No file loaded".to_string())),
        }
    };

    // 异步重建索引
    if result.is_ok() {
        spawn_rebuild_sheet_index(sheet_index, state.clone());
    }

    result
}

/// 添加列
pub fn do_add_column(state: Arc<RwLock<Option<EditorState>>>, sheet_index: usize) -> Result<OperationResult, AppError> {
    let result = {
        let mut state_guard = state.write().unwrap();
        match state_guard.as_mut() {
            Some(editor_state) => {
                let operation = crate::command::Operation::AddColumn { sheet_index };
                Ok(editor_state.execute(operation))
            }
            None => Err(AppError::Internal("No file loaded".to_string())),
        }
    };

    // 异步重建索引
    if result.is_ok() {
        spawn_rebuild_sheet_index(sheet_index, state.clone());
    }

    result
}

/// 删除列
pub fn do_delete_column(state: Arc<RwLock<Option<EditorState>>>, sheet_index: usize, col_index: usize, col_data: Vec<CellValue>) -> Result<OperationResult, AppError> {
    let result = {
        let mut state_guard = state.write().unwrap();
        match state_guard.as_mut() {
            Some(editor_state) => {
                let operation = crate::command::Operation::DeleteColumn {
                    sheet_index,
                    col_index,
                    col_data,
                };
                Ok(editor_state.execute(operation))
            }
            None => Err(AppError::Internal("No file loaded".to_string())),
        }
    };

    // 异步重建索引
    if result.is_ok() {
        spawn_rebuild_sheet_index(sheet_index, state.clone());
    }

    result
}

/// 添加 Sheet
pub fn do_add_sheet(state: Arc<RwLock<Option<EditorState>>>) -> Result<OperationResult, AppError> {
    let result = {
        let mut state_guard = state.write().unwrap();
        match state_guard.as_mut() {
            Some(editor_state) => {
                let operation = crate::command::Operation::AddSheet;
                Ok(editor_state.execute(operation))
            }
            None => Err(AppError::Internal("No file loaded".to_string())),
        }
    };

    // Note: Adding a sheet doesn't require index rebuild since it's a new empty sheet

    result
}

/// 删除 Sheet
pub fn do_delete_sheet(state: Arc<RwLock<Option<EditorState>>>, sheet_index: usize) -> Result<OperationResult, AppError> {
    let result = {
        let mut state_guard = state.write().unwrap();
        match state_guard.as_mut() {
            Some(editor_state) => {
                let operation = crate::command::Operation::DeleteSheet { sheet_index };
                Ok(editor_state.execute(operation))
            }
            None => Err(AppError::Internal("No file loaded".to_string())),
        }
    };

    result
}
