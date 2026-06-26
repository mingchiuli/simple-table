use std::sync::{Arc, RwLock};

use crate::error::AppError;
use crate::ops::Operation;
use crate::ops::editor_ops::{cell_delta_mutation_response, snapshot_mutation_response};
use crate::ops::index_ops::{spawn_rebuild_all_sheets_index, spawn_update_cell_index};
use crate::state::editor_state::EditorState;
use crate::types::{CellValue, EditorMutationResponse, SheetData};

/// 设置单元格值
pub fn do_set_cell(
    state: Arc<RwLock<Option<EditorState>>>,
    sheet_index: usize,
    row: usize,
    col: usize,
    old_value: CellValue,
    new_value: CellValue,
) -> Result<EditorMutationResponse, AppError> {
    let response = {
        let mut state_guard = state.write().expect("Editor state lock poisoned");
        match state_guard.as_mut() {
            Some(editor_state) => {
                let sheet = editor_state
                    .file_data()
                    .sheets
                    .get(sheet_index)
                    .ok_or(AppError::InvalidSheetIndex(sheet_index))?;
                if row >= sheet.rows.len() || sheet.rows.get(row).is_none_or(|r| col >= r.len()) {
                    return Err(AppError::InvalidCellPosition { row, col });
                }
                let operation = Operation::SetCell {
                    sheet_index,
                    row,
                    col,
                    old_value,
                    new_value: new_value.clone(),
                };
                let result = editor_state.execute(operation);
                Ok(cell_delta_mutation_response(
                    editor_state,
                    result.operation,
                    result.cell_changes,
                ))
            }
            None => Err(AppError::NoFileLoaded),
        }
    };

    if let Ok(response) = &response {
        for change in &response.cell_changes {
            spawn_update_cell_index(
                change.sheet_index,
                change.row,
                change.col,
                &change.value,
                state.clone(),
            );
        }
    }

    response
}

/// 添加行
pub fn do_add_row(
    state: Arc<RwLock<Option<EditorState>>>,
    sheet_index: usize,
    row_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let response = {
        let mut state_guard = state.write().expect("Editor state lock poisoned");
        match state_guard.as_mut() {
            Some(editor_state) => {
                if sheet_index >= editor_state.file_data().sheets.len() {
                    return Err(AppError::InvalidSheetIndex(sheet_index));
                }
                if row_index > editor_state.file_data().sheets[sheet_index].rows.len() {
                    return Err(AppError::RowNotFound(row_index));
                }
                // 直接计算 row_data（空行数据）
                let operation = Operation::AddRow {
                    sheet_index,
                    row_index,
                    row_data: vec![],
                };
                let result = editor_state.execute(operation);
                Ok(snapshot_mutation_response(
                    editor_state,
                    Some(result.operation),
                ))
            }
            None => Err(AppError::NoFileLoaded),
        }
    };

    if response.is_ok() {
        spawn_rebuild_all_sheets_index(state);
    }

    response
}

/// 删除行
pub fn do_delete_row(
    state: Arc<RwLock<Option<EditorState>>>,
    sheet_index: usize,
    row_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let response = {
        let mut state_guard = state.write().expect("Editor state lock poisoned");
        match state_guard.as_mut() {
            Some(editor_state) => {
                let sheet = editor_state
                    .file_data()
                    .sheets
                    .get(sheet_index)
                    .ok_or(AppError::InvalidSheetIndex(sheet_index))?;
                // 从文件数据中获取行数据（用于撤销）
                let row_data = sheet
                    .rows
                    .get(row_index)
                    .cloned()
                    .ok_or(AppError::RowNotFound(row_index))?;
                let operation = Operation::DeleteRow {
                    sheet_index,
                    row_index,
                    row_data,
                };
                let result = editor_state.execute(operation);
                Ok(snapshot_mutation_response(
                    editor_state,
                    Some(result.operation),
                ))
            }
            None => Err(AppError::NoFileLoaded),
        }
    };

    if response.is_ok() {
        spawn_rebuild_all_sheets_index(state);
    }

    response
}

/// 添加列
pub fn do_add_column(
    state: Arc<RwLock<Option<EditorState>>>,
    sheet_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let response = {
        let mut state_guard = state.write().expect("Editor state lock poisoned");
        match state_guard.as_mut() {
            Some(editor_state) => {
                if sheet_index >= editor_state.file_data().sheets.len() {
                    return Err(AppError::InvalidSheetIndex(sheet_index));
                }
                // col_index 和 col_data 会在 execute 中自动计算和保存
                let operation = Operation::AddColumn {
                    sheet_index,
                    col_index: None,
                    col_data: vec![],
                };
                let result = editor_state.execute(operation);
                Ok(snapshot_mutation_response(
                    editor_state,
                    Some(result.operation),
                ))
            }
            None => Err(AppError::NoFileLoaded),
        }
    };

    if response.is_ok() {
        spawn_rebuild_all_sheets_index(state);
    }

    response
}

/// 删除列
pub fn do_delete_column(
    state: Arc<RwLock<Option<EditorState>>>,
    sheet_index: usize,
    col_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let response = {
        let mut state_guard = state.write().expect("Editor state lock poisoned");
        match state_guard.as_mut() {
            Some(editor_state) => {
                let sheet = editor_state
                    .file_data()
                    .sheets
                    .get(sheet_index)
                    .ok_or(AppError::InvalidSheetIndex(sheet_index))?;
                let total_cols = sheet.rows.first().map(|r| r.len()).unwrap_or(0);
                if col_index >= total_cols {
                    return Err(AppError::InvalidCellPosition {
                        row: 0,
                        col: col_index,
                    });
                }
                // 从文件数据中获取列数据（用于撤销）
                let col_data: Vec<CellValue> = sheet
                    .rows
                    .iter()
                    .map(|row| row.get(col_index).cloned().unwrap_or(CellValue::Null))
                    .collect();
                let operation = Operation::DeleteColumn {
                    sheet_index,
                    col_index,
                    col_data,
                };
                let result = editor_state.execute(operation);
                Ok(snapshot_mutation_response(
                    editor_state,
                    Some(result.operation),
                ))
            }
            None => Err(AppError::NoFileLoaded),
        }
    };

    if response.is_ok() {
        spawn_rebuild_all_sheets_index(state);
    }

    response
}

pub fn do_set_column_width(
    state: Arc<RwLock<Option<EditorState>>>,
    sheet_index: usize,
    col_index: usize,
    width: Option<u32>,
) -> Result<EditorMutationResponse, AppError> {
    let response = {
        let mut state_guard = state.write().expect("Editor state lock poisoned");
        match state_guard.as_mut() {
            Some(editor_state) => {
                let sheet = editor_state
                    .file_data()
                    .sheets
                    .get(sheet_index)
                    .ok_or(AppError::InvalidSheetIndex(sheet_index))?;
                let old_width = sheet
                    .column_widths
                    .as_ref()
                    .and_then(|widths| widths.get(&col_index).copied());
                let operation = Operation::SetColumnWidth {
                    sheet_index,
                    col_index,
                    old_width,
                    new_width: width,
                };
                let result = editor_state.execute(operation);
                Ok(snapshot_mutation_response(
                    editor_state,
                    Some(result.operation),
                ))
            }
            None => Err(AppError::NoFileLoaded),
        }
    };

    response
}

pub fn do_set_row_height(
    state: Arc<RwLock<Option<EditorState>>>,
    sheet_index: usize,
    row_index: usize,
    height: Option<u32>,
) -> Result<EditorMutationResponse, AppError> {
    let response = {
        let mut state_guard = state.write().expect("Editor state lock poisoned");
        match state_guard.as_mut() {
            Some(editor_state) => {
                let sheet = editor_state
                    .file_data()
                    .sheets
                    .get(sheet_index)
                    .ok_or(AppError::InvalidSheetIndex(sheet_index))?;
                let old_height = sheet
                    .row_heights
                    .as_ref()
                    .and_then(|heights| heights.get(&row_index).copied());
                let operation = Operation::SetRowHeight {
                    sheet_index,
                    row_index,
                    old_height,
                    new_height: height,
                };
                let result = editor_state.execute(operation);
                Ok(snapshot_mutation_response(
                    editor_state,
                    Some(result.operation),
                ))
            }
            None => Err(AppError::NoFileLoaded),
        }
    };

    response
}

/// 添加 Sheet
pub fn do_add_sheet(
    state: Arc<RwLock<Option<EditorState>>>,
) -> Result<EditorMutationResponse, AppError> {
    let response = {
        let mut state_guard = state.write().expect("Editor state lock poisoned");
        match state_guard.as_mut() {
            Some(editor_state) => {
                // 传入空字符串和 None，让 execute 生成名称并创建空 sheet
                let operation = Operation::AddSheet {
                    name: String::new(),
                    sheet_data: None,
                    sheet_index: None,
                };
                let result = editor_state.execute(operation);
                Ok(snapshot_mutation_response(
                    editor_state,
                    Some(result.operation),
                ))
            }
            None => Err(AppError::NoFileLoaded),
        }
    };

    if response.is_ok() {
        spawn_rebuild_all_sheets_index(state);
    }

    response
}

/// 删除 Sheet
pub fn do_delete_sheet(
    state: Arc<RwLock<Option<EditorState>>>,
    sheet_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let response = {
        let mut state_guard = state.write().expect("Editor state lock poisoned");
        match state_guard.as_mut() {
            Some(editor_state) => {
                if editor_state.file_data().sheets.len() <= 1 {
                    return Err(AppError::CannotDeleteLastSheet);
                }
                if sheet_index >= editor_state.file_data().sheets.len() {
                    return Err(AppError::InvalidSheetIndex(sheet_index));
                }
                // sheet_data 为空，会在 execute 中自动保存
                let operation = Operation::DeleteSheet {
                    sheet_index,
                    sheet_data: SheetData::default(),
                };
                let result = editor_state.execute(operation);
                Ok(snapshot_mutation_response(
                    editor_state,
                    Some(result.operation),
                ))
            }
            None => Err(AppError::NoFileLoaded),
        }
    };

    if response.is_ok() {
        spawn_rebuild_all_sheets_index(state);
    }

    response
}
