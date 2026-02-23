use std::sync::Arc;
use std::sync::RwLock;

use crate::command::EditorState;
use crate::error::AppError;
use crate::types::{SearchResult, SearchScope};

/// 将列索引转换为字母 (0 -> A, 1 -> B, ...)
fn col_to_letter(col: usize) -> String {
    let mut result = String::new();
    let mut n = col;
    while n >= 26 {
        result.insert(0, char::from_u32((n % 26) as u32 + 65).unwrap());
        n = n / 26 - 1;
    }
    result.insert(0, char::from_u32(n as u32 + 65).unwrap());
    result
}

/// 将单元格值转换为字符串
fn cell_to_string(cell: &crate::types::CellValue) -> String {
    match cell {
        crate::types::CellValue::Null => String::new(),
        crate::types::CellValue::String(s) => s.clone(),
        crate::types::CellValue::Number(n) => n.to_string(),
        crate::types::CellValue::Boolean(b) => b.to_string(),
    }
}

/// 搜索单元格
pub fn do_search(
    state: Arc<RwLock<Option<EditorState>>>,
    query: String,
    scope: SearchScope,
    current_sheet_index: Option<usize>,
) -> Result<Vec<SearchResult>, AppError> {
    if query.is_empty() {
        return Ok(vec![]);
    }

    let token = query.to_lowercase();
    let state = state.read().unwrap();

    let editor_state = match state.as_ref() {
        Some(s) => s,
        None => return Err(AppError::Internal("No file loaded".to_string())),
    };

    let mut results = Vec::new();

    match scope {
        SearchScope::CurrentSheet => {
            let sheet_idx = current_sheet_index.unwrap_or(0);
            if let Some(sheet) = editor_state.file_data.sheets.get(sheet_idx) {
                if let Some(positions) = sheet.index.inverted_index.get(&token) {
                    for pos in positions {
                        let value = sheet.rows.get(pos.row)
                            .and_then(|r| r.get(pos.col))
                            .map(|c| cell_to_string(c))
                            .unwrap_or_default();

                        results.push(SearchResult {
                            sheet_index: sheet_idx,
                            sheet_name: sheet.name.clone(),
                            row: pos.row,
                            col: pos.col,
                            value,
                            cell_position: format!("{}{}", col_to_letter(pos.col), pos.row + 1),
                        });
                    }
                }
            }
        }
        SearchScope::AllSheets => {
            for (sheet_idx, sheet) in editor_state.file_data.sheets.iter().enumerate() {
                if let Some(positions) = sheet.index.inverted_index.get(&token) {
                    for pos in positions {
                        let value = sheet.rows.get(pos.row)
                            .and_then(|r| r.get(pos.col))
                            .map(|c| cell_to_string(c))
                            .unwrap_or_default();

                        results.push(SearchResult {
                            sheet_index: sheet_idx,
                            sheet_name: sheet.name.clone(),
                            row: pos.row,
                            col: pos.col,
                            value,
                            cell_position: format!("{}{}", col_to_letter(pos.col), pos.row + 1),
                        });
                    }
                }
            }
        }
    }

    Ok(results)
}
