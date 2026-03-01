use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

use crate::editor_state::EditorState;
use crate::types::{CellPosition, CellValue, SheetData};

/// 将单元格值转换为字符串
fn cell_to_string(cell: &CellValue) -> String {
    match cell {
        CellValue::Null => String::new(),
        CellValue::String(s) => s.clone(),
        CellValue::Number(n) => n.to_string(),
        CellValue::Boolean(b) => b.to_string(),
    }
}

/// 重建单个 sheet 的索引
pub fn rebuild_sheet_index(sheet: &mut SheetData) {
    let mut inverted_index: HashMap<String, Vec<CellPosition>> = HashMap::new();

    for (row_idx, row) in sheet.rows.iter().enumerate() {
        for (col_idx, cell) in row.iter().enumerate() {
            let text = cell_to_string(cell);
            if !text.is_empty() {
                let token = text.to_lowercase();
                inverted_index
                    .entry(token)
                    .or_default()
                    .push(CellPosition {
                        row: row_idx,
                        col: col_idx,
                    });
            }
        }
    }

    sheet.index.inverted_index = inverted_index;
}

/// 异步重建指定 sheet 的索引
pub fn spawn_rebuild_sheet_index(sheet_index: usize, state: Arc<RwLock<Option<EditorState>>>) {
    std::thread::spawn(move || {
        if let Ok(mut guard) = state.write() {
            if let Some(ref mut editor_state) = *guard {
                if let Some(sheet) = editor_state.file_data.sheets.get_mut(sheet_index) {
                    rebuild_sheet_index(sheet);
                }
            }
        }
    });
}

/// 异步重建所有 sheets 的索引
pub fn spawn_rebuild_all_sheets_index(state: Arc<RwLock<Option<EditorState>>>) {
    std::thread::spawn(move || {
        if let Ok(mut guard) = state.write() {
            if let Some(ref mut editor_state) = *guard {
                for sheet in &mut editor_state.file_data.sheets {
                    rebuild_sheet_index(sheet);
                }
            }
        }
    });
}
