use std::sync::{Arc, RwLock};

use crate::error::AppError;
use crate::ops::index_ops::{
    spawn_append_column_index, spawn_append_row_index, spawn_delete_last_column_index,
    spawn_delete_last_row_index, spawn_rebuild_sheet_index, spawn_update_cell_index,
};
use crate::state::editor_state::EditorState;
use crate::ops::Operation;
use crate::types::{CellValue, SheetData};

/// 设置单元格值
pub fn do_set_cell(
    state: Arc<RwLock<Option<EditorState>>>,
    sheet_index: usize,
    row: usize,
    col: usize,
    old_value: CellValue,
    new_value: CellValue,
) -> Result<(), AppError> {
    let result = {
        let mut state_guard = state.write().expect("Editor state lock poisoned");
        match state_guard.as_mut() {
            Some(editor_state) => {
                let operation = Operation::SetCell {
                    sheet_index,
                    row,
                    col,
                    old_value,
                    new_value: new_value.clone(),
                };
                editor_state.execute(operation);
                Ok(())
            }
            None => Err(AppError::NoFileLoaded),
        }
    };

    // 增量更新单格索引（worker 内部 delete_term + add_document）
    if result.is_ok() {
        spawn_update_cell_index(sheet_index, row, col, &new_value, state.clone());
    }

    result
}

/// 添加行
pub fn do_add_row(
    state: Arc<RwLock<Option<EditorState>>>,
    sheet_index: usize,
    row_index: usize,
) -> Result<(), AppError> {
    // 在 execute 之前判断是否末尾追加（前端唯一调用路径），用于选择增量分支
    let is_append_at_end = {
        let guard = state.read().expect("Editor state lock poisoned");
        guard
            .as_ref()
            .and_then(|s| s.file_data.sheets.get(sheet_index))
            .map(|sh| row_index == sh.rows.len())
            .unwrap_or(false)
    };

    let result = {
        let mut state_guard = state.write().expect("Editor state lock poisoned");
        match state_guard.as_mut() {
            Some(editor_state) => {
                // 直接计算 row_data（空行数据）
                let operation = Operation::AddRow {
                    sheet_index,
                    row_index,
                    row_data: vec![],
                };
                editor_state.execute(operation);
                Ok(())
            }
            None => Err(AppError::NoFileLoaded),
        }
    };

    if result.is_ok() {
        if is_append_at_end {
            // execute 内部会把空 row_data 填成 vec![Null; col_count]，索引层全空 → 空提交
            spawn_append_row_index(sheet_index, row_index, vec![], state.clone());
        } else {
            spawn_rebuild_sheet_index(sheet_index, state.clone());
        }
    }

    result
}

/// 删除行
pub fn do_delete_row(
    state: Arc<RwLock<Option<EditorState>>>,
    sheet_index: usize,
    row_index: usize,
) -> Result<(), AppError> {
    let mut col_count: usize = 0;
    let mut is_last_row = false;
    let result = {
        let mut state_guard = state.write().expect("Editor state lock poisoned");
        match state_guard.as_mut() {
            Some(editor_state) => {
                let sheet = editor_state
                    .file_data
                    .sheets
                    .get(sheet_index)
                    .ok_or_else(|| AppError::RowNotFound(row_index))?;
                is_last_row = row_index + 1 == sheet.rows.len();
                col_count = sheet.rows.first().map(|r| r.len()).unwrap_or(0);
                // 从文件数据中获取行数据（用于撤销）
                let row_data = sheet
                    .rows
                    .get(row_index)
                    .cloned()
                    .ok_or_else(|| AppError::RowNotFound(row_index))?;
                let operation = Operation::DeleteRow {
                    sheet_index,
                    row_index,
                    row_data,
                };
                editor_state.execute(operation);
                Ok(())
            }
            None => Err(AppError::NoFileLoaded),
        }
    };

    if result.is_ok() {
        if is_last_row {
            spawn_delete_last_row_index(sheet_index, row_index, col_count, state.clone());
        } else {
            spawn_rebuild_sheet_index(sheet_index, state.clone());
        }
    }

    result
}

/// 添加列
pub fn do_add_column(
    state: Arc<RwLock<Option<EditorState>>>,
    sheet_index: usize,
) -> Result<(), AppError> {
    // 用户路径下永远末尾追加；记录追加位置用于增量
    let new_col_index = {
        let guard = state.read().expect("Editor state lock poisoned");
        guard
            .as_ref()
            .and_then(|s| s.file_data.sheets.get(sheet_index))
            .and_then(|sh| sh.rows.first())
            .map(|r| r.len())
    };

    let result = {
        let mut state_guard = state.write().expect("Editor state lock poisoned");
        match state_guard.as_mut() {
            Some(editor_state) => {
                // col_index 和 col_data 会在 execute 中自动计算和保存
                let operation = Operation::AddColumn {
                    sheet_index,
                    col_index: None,
                    col_data: vec![],
                };
                editor_state.execute(operation);
                Ok(())
            }
            None => Err(AppError::NoFileLoaded),
        }
    };

    if result.is_ok() {
        if let Some(col_index) = new_col_index {
            // 新列全空，索引层无需写入文档；增量提交开销 ~ms
            spawn_append_column_index(sheet_index, col_index, vec![], state.clone());
        } else {
            spawn_rebuild_sheet_index(sheet_index, state.clone());
        }
    }

    result
}

/// 删除列
pub fn do_delete_column(
    state: Arc<RwLock<Option<EditorState>>>,
    sheet_index: usize,
    col_index: usize,
) -> Result<(), AppError> {
    let mut row_count: usize = 0;
    let mut is_last_col = false;
    let result = {
        let mut state_guard = state.write().expect("Editor state lock poisoned");
        match state_guard.as_mut() {
            Some(editor_state) => {
                let sheet = &editor_state.file_data.sheets[sheet_index];
                let total_cols = sheet.rows.first().map(|r| r.len()).unwrap_or(0);
                is_last_col = col_index + 1 == total_cols;
                row_count = sheet.rows.len();
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
                editor_state.execute(operation);
                Ok(())
            }
            None => Err(AppError::NoFileLoaded),
        }
    };

    if result.is_ok() {
        if is_last_col {
            spawn_delete_last_column_index(sheet_index, col_index, row_count, state.clone());
        } else {
            spawn_rebuild_sheet_index(sheet_index, state.clone());
        }
    }

    result
}

/// 添加 Sheet
pub fn do_add_sheet(state: Arc<RwLock<Option<EditorState>>>) -> Result<(), AppError> {
    let result = {
        let mut state_guard = state.write().expect("Editor state lock poisoned");
        match state_guard.as_mut() {
            Some(editor_state) => {
                // 传入空字符串和 None，让 execute 生成名称并创建空 sheet
                let operation = Operation::AddSheet {
                    name: String::new(),
                    sheet_data: None,
                    sheet_index: None,
                };
                editor_state.execute(operation);
                Ok(())
            }
            None => Err(AppError::NoFileLoaded),
        }
    };

    // Note: Adding a sheet doesn't require index rebuild since it's a new empty sheet

    result
}

/// 删除 Sheet
pub fn do_delete_sheet(
    state: Arc<RwLock<Option<EditorState>>>,
    sheet_index: usize,
) -> Result<(), AppError> {
    let mut state_guard = state.write().expect("Editor state lock poisoned");
    match state_guard.as_mut() {
        Some(editor_state) => {
            if editor_state.file_data.sheets.len() <= 1 {
                return Err(AppError::CannotDeleteLastSheet);
            }
            if sheet_index >= editor_state.file_data.sheets.len() {
                return Err(AppError::InvalidSheetIndex(sheet_index));
            }
            // sheet_data 为空，会在 execute 中自动保存
            let operation = Operation::DeleteSheet {
                sheet_index,
                sheet_data: SheetData::default(),
            };
            editor_state.execute(operation);
            Ok(())
        }
        None => Err(AppError::NoFileLoaded),
    }
}
