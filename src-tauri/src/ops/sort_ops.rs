use std::sync::{Arc, RwLock};

use crate::error::AppError;
use crate::ops::Operation;
use crate::ops::index_ops::spawn_rebuild_sheet_index;
use crate::state::editor_state::EditorState;
use crate::types::{OperationResult, SortState};

/// 对指定列进行排序
pub fn do_sort_column(
    state: Arc<RwLock<Option<EditorState>>>,
    sheet_index: usize,
    col_index: usize,
    ascending: bool,
    previous_sort_state: Option<SortState>,
) -> Result<OperationResult, AppError> {
    let (result, needs_rebuild) = {
        let mut state = state.write().expect("Editor state lock poisoned");
        match state.as_mut() {
            Some(editor_state) => {
                // 获取当前 sheet 数据（排序前）
                let old_sheet_data = editor_state
                    .file_data
                    .sheets
                    .get(sheet_index)
                    .cloned()
                    .ok_or(AppError::InvalidSheetIndex(sheet_index))?;
                let col_count = old_sheet_data.rows.first().map(|r| r.len()).unwrap_or(0);
                if col_index >= col_count {
                    return Err(AppError::InvalidCellPosition {
                        row: 0,
                        col: col_index,
                    });
                }

                let operation = Operation::SortColumn {
                    sheet_index,
                    col_index,
                    ascending,
                    old_sheet_data,
                    previous_sort_state,
                };
                let result = editor_state.execute(operation);
                (result, true)
            }
            None => return Err(AppError::NoFileLoaded),
        }
    };

    // 异步重建索引
    if needs_rebuild {
        spawn_rebuild_sheet_index(sheet_index, state);
    }

    Ok(result)
}
