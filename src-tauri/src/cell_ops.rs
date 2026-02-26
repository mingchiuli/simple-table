use std::sync::Arc;
use std::sync::RwLock;

use crate::editor_state::EditorState;
use crate::error::AppError;
use crate::types::{CellValue, SheetData};

/// 异步重建指定 sheet 的索引
fn spawn_rebuild_sheet_index(sheet_index: usize, state: Arc<RwLock<Option<EditorState>>>) {
    std::thread::spawn(move || {
        if let Ok(mut guard) = state.write() {
            if let Some(ref mut editor_state) = *guard {
                if let Some(sheet) = editor_state.file_data.sheets.get_mut(sheet_index) {
                    crate::editor_state::rebuild_sheet_index(sheet);
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
) -> Result<(), AppError> {
    let mut state = state.write().unwrap();
    match state.as_mut() {
        Some(editor_state) => {
            let operation = crate::editor_state::Operation::SetCell {
                sheet_index,
                row,
                col,
                old_value,
                new_value,
            };
            editor_state.execute(operation);
            Ok(())
        }
        None => Err(AppError::Internal("No file loaded".to_string())),
    }
}

/// 添加行
pub fn do_add_row(state: Arc<RwLock<Option<EditorState>>>, sheet_index: usize, row_index: usize) -> Result<(), AppError> {
    let result = {
        let mut state_guard = state.write().unwrap();
        match state_guard.as_mut() {
            Some(editor_state) => {
                let operation = crate::editor_state::Operation::AddRow {
                    sheet_index,
                    row_index,
                };
                editor_state.execute(operation);
                Ok(())
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
pub fn do_delete_row(state: Arc<RwLock<Option<EditorState>>>, sheet_index: usize, row_index: usize) -> Result<(), AppError> {
    let result = {
        let mut state_guard = state.write().unwrap();
        match state_guard.as_mut() {
            Some(editor_state) => {
                // 从文件数据中获取行数据（用于撤销）
                let row_data = editor_state.file_data.sheets[sheet_index].rows[row_index].clone();
                let operation = crate::editor_state::Operation::DeleteRow {
                    sheet_index,
                    row_index,
                    row_data,
                };
                editor_state.execute(operation);
                Ok(())
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
pub fn do_add_column(state: Arc<RwLock<Option<EditorState>>>, sheet_index: usize) -> Result<(), AppError> {
    let result = {
        let mut state_guard = state.write().unwrap();
        match state_guard.as_mut() {
            Some(editor_state) => {
                // col_index 和 col_data 会在 execute 中自动计算和保存
                let operation = crate::editor_state::Operation::AddColumn { sheet_index, col_index: None, col_data: vec![] };
                editor_state.execute(operation);
                Ok(())
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
pub fn do_delete_column(state: Arc<RwLock<Option<EditorState>>>, sheet_index: usize, col_index: usize) -> Result<(), AppError> {
    let result = {
        let mut state_guard = state.write().unwrap();
        match state_guard.as_mut() {
            Some(editor_state) => {
                // 从文件数据中获取列数据（用于撤销）
                let col_data: Vec<CellValue> = editor_state.file_data.sheets[sheet_index]
                    .rows
                    .iter()
                    .map(|row| row.get(col_index).cloned().unwrap_or(CellValue::Null))
                    .collect();
                let operation = crate::editor_state::Operation::DeleteColumn {
                    sheet_index,
                    col_index,
                    col_data,
                };
                editor_state.execute(operation);
                Ok(())
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
pub fn do_add_sheet(state: Arc<RwLock<Option<EditorState>>>) -> Result<(), AppError> {
    let result = {
        let mut state_guard = state.write().unwrap();
        match state_guard.as_mut() {
            Some(editor_state) => {
                // 传入空字符串和 None，让 execute 生成名称并创建空 sheet
                let operation = crate::editor_state::Operation::AddSheet {
                    name: String::new(),
                    sheet_data: None,
                };
                editor_state.execute(operation);
                Ok(())
            }
            None => Err(AppError::Internal("No file loaded".to_string())),
        }
    };

    // Note: Adding a sheet doesn't require index rebuild since it's a new empty sheet

    result
}

/// 删除 Sheet
pub fn do_delete_sheet(state: Arc<RwLock<Option<EditorState>>>, sheet_index: usize) -> Result<(), AppError> {
    let result = {
        let mut state_guard = state.write().unwrap();
        match state_guard.as_mut() {
            Some(editor_state) => {
                // sheet_data 为空，会在 execute 中自动保存
                let operation = crate::editor_state::Operation::DeleteSheet {
                    sheet_index,
                    sheet_data: SheetData::default(),
                };
                editor_state.execute(operation);
                Ok(())
            }
            None => Err(AppError::Internal("No file loaded".to_string())),
        }
    };

    result
}
