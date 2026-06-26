use std::sync::{Arc, RwLock};

use crate::error::AppError;
use crate::state::editor_state::EditorState;
use crate::types::{SearchResult, SearchScope};

/// 将列索引转换为字母 (0 -> A, 1 -> B, ...)
fn col_to_letter(col: usize) -> String {
    let mut result = String::new();
    let mut n = col;
    while n >= 26 {
        // Safety: math guarantees result is ASCII uppercase letter (65-90)
        result.insert(
            0,
            char::from_u32((n % 26) as u32 + 65).expect("Invalid ASCII letter"),
        );
        n = n / 26 - 1;
    }
    // Safety: math guarantees result is ASCII uppercase letter (65-90)
    result.insert(
        0,
        char::from_u32(n as u32 + 65).expect("Invalid ASCII letter"),
    );
    result
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

    let state = state.read().expect("Editor state lock poisoned");

    let editor_state = match state.as_ref() {
        Some(s) => s,
        None => return Err(AppError::NoFileLoaded),
    };

    let mut results = Vec::new();

    match scope {
        SearchScope::CurrentSheet => {
            let sheet_idx = current_sheet_index.unwrap_or(0);
            if let Some(sheet) = editor_state.file_data().sheets.get(sheet_idx) {
                let positions = editor_state.search_sheet(sheet_idx, &query, 1000);
                for pos in positions {
                    let value = sheet
                        .rows
                        .get(pos.row)
                        .and_then(|r| r.get(pos.col))
                        .map(|c| c.to_display_string())
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
        SearchScope::AllSheets => {
            for (sheet_idx, sheet) in editor_state.file_data().sheets.iter().enumerate() {
                let positions = editor_state.search_sheet(sheet_idx, &query, 1000);
                for pos in positions {
                    let value = sheet
                        .rows
                        .get(pos.row)
                        .and_then(|r| r.get(pos.col))
                        .map(|c| c.to_display_string())
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

    Ok(results)
}
