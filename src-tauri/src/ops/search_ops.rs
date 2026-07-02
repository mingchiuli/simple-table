use std::sync::{Arc, RwLock};

use crate::error::AppError;
use crate::state::editor_state::{SearchCellSnapshot, SearchSheetSnapshot};
use crate::state::search_index::SearchMatcher;
use crate::state::state::ActiveDocumentStore;
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
    registry: Arc<RwLock<ActiveDocumentStore>>,
    query: String,
    scope: SearchScope,
    current_sheet_index: Option<usize>,
) -> Result<Vec<SearchResult>, AppError> {
    if query.is_empty() {
        return Ok(vec![]);
    }

    let search_inputs = {
        let registry = registry.read().expect("Document registry lock poisoned");
        let editor_state = match registry.active() {
            Some(s) => s,
            None => return Err(AppError::NoFileLoaded),
        };

        match scope {
            SearchScope::CurrentSheet => {
                let sheet_idx = current_sheet_index.unwrap_or(0);
                vec![search_input_for_sheet(editor_state, sheet_idx, &query)]
            }
            SearchScope::AllSheets => editor_state
                .file_data()
                .sheets
                .iter()
                .enumerate()
                .map(|(sheet_idx, _)| search_input_for_sheet(editor_state, sheet_idx, &query))
                .collect(),
        }
    };

    let mut results = Vec::new();
    let mut used_scan_fallback = false;

    for input in search_inputs {
        let Some(input) = input else {
            continue;
        };
        match input {
            SearchInput::Indexed {
                sheet_index,
                sheet_name,
                cells,
            } => {
                for cell in cells {
                    results.push(SearchResult {
                        sheet_index,
                        sheet_name: sheet_name.clone(),
                        row: cell.row,
                        col: cell.col,
                        value: cell.text,
                        cell_position: format!("{}{}", col_to_letter(cell.col), cell.row + 1),
                    });
                }
            }
            SearchInput::Scan(snapshot) => {
                used_scan_fallback = true;
                for cell in scan_sheet_snapshot(&snapshot, &query, 1000) {
                    results.push(SearchResult {
                        sheet_index: snapshot.sheet_index,
                        sheet_name: snapshot.sheet_name.clone(),
                        row: cell.row,
                        col: cell.col,
                        value: cell.text,
                        cell_position: format!("{}{}", col_to_letter(cell.col), cell.row + 1),
                    });
                }
            }
        }
    }

    if used_scan_fallback {
        eprintln!("Search used synchronous scan fallback while index was stale or unavailable");
    }

    Ok(results)
}

enum SearchInput {
    Indexed {
        sheet_index: usize,
        sheet_name: String,
        cells: Vec<SearchCellSnapshot>,
    },
    Scan(SearchSheetSnapshot),
}

fn search_input_for_sheet(
    editor_state: &crate::state::editor_state::EditorState,
    sheet_index: usize,
    query: &str,
) -> Option<SearchInput> {
    if let Some(cells) = editor_state.indexed_search_sheet(sheet_index, query, 1000) {
        return Some(SearchInput::Indexed {
            sheet_index,
            sheet_name: editor_state.sheet_name(sheet_index)?,
            cells,
        });
    }
    Some(SearchInput::Scan(
        editor_state.search_sheet_snapshot(sheet_index)?,
    ))
}

fn scan_sheet_snapshot(
    snapshot: &SearchSheetSnapshot,
    query: &str,
    limit: usize,
) -> Vec<SearchCellSnapshot> {
    let Some(matcher) = SearchMatcher::new(query) else {
        return Vec::new();
    };
    let mut results = Vec::new();
    for SearchCellSnapshot { row, col, text } in &snapshot.cells {
        if matcher.matches(text) {
            results.push(SearchCellSnapshot {
                row: *row,
                col: *col,
                text: text.clone(),
            });
            if results.len() >= limit {
                break;
            }
        }
    }
    results
}
