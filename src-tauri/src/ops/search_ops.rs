use std::sync::{Arc, RwLock};

use crate::error::AppError;
use crate::state::search_index::{SearchCellText, SearchMatcher, collect_sheet_search_text};
use crate::state::state::ActiveDocumentStore;
use crate::types::{SearchResult, SearchScope};

const SEARCH_RESULT_LIMIT: usize = 1000;

/// 将列索引转换为字母 (0 -> A, 1 -> B, ...)
fn col_to_letter(col: usize) -> String {
    let mut result = String::new();
    let mut n = col;
    while n >= 26 {
        result.insert(0, (b'A' + (n % 26) as u8) as char);
        n = n / 26 - 1;
    }
    result.insert(0, (b'A' + n as u8) as char);
    result
}

/// 搜索单元格
pub fn do_search(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    query: &str,
    scope: SearchScope,
    current_sheet_index: Option<usize>,
) -> Result<Vec<SearchResult>, AppError> {
    if query.is_empty() {
        return Ok(vec![]);
    }

    let sheet_indexes = {
        let registry = registry
            .read()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        let editor_state = match registry.active() {
            Some(s) => s,
            None => return Err(AppError::NoFileLoaded),
        };

        match scope {
            SearchScope::CurrentSheet => vec![current_sheet_index.unwrap_or(0)],
            SearchScope::AllSheets => editor_state
                .file_data()
                .sheets
                .iter()
                .enumerate()
                .map(|(sheet_idx, _)| sheet_idx)
                .collect(),
        }
    };

    let matcher = SearchMatcher::new(query);
    let mut results = Vec::new();
    let mut used_scan_fallback = false;

    for sheet_index in sheet_indexes {
        if results.len() >= SEARCH_RESULT_LIMIT {
            break;
        }
        let remaining = SEARCH_RESULT_LIMIT - results.len();
        let input = {
            let registry = registry
                .read()
                .map_err(|_| AppError::poisoned_lock("document registry"))?;
            let Some(editor_state) = registry.active() else {
                return Err(AppError::NoFileLoaded);
            };
            search_input_for_sheet(
                editor_state,
                sheet_index,
                query,
                matcher.as_ref(),
                remaining,
            )
        };
        let Some(input) = input else {
            continue;
        };
        match input {
            SearchInput::Indexed {
                sheet_index,
                sheet_name,
                cells,
            } => {
                for cell in cells.into_iter().take(remaining) {
                    results.push(SearchResult {
                        sheet_index,
                        sheet_name: sheet_name.clone(),
                        row: cell.row,
                        col: cell.col,
                        value: cell.display_text,
                        cell_position: format!("{}{}", col_to_letter(cell.col), cell.row + 1),
                    });
                }
            }
            SearchInput::Scan {
                sheet_index,
                sheet_name,
                cells,
            } => {
                used_scan_fallback = true;
                for cell in scan_sheet(&cells, matcher.as_ref(), remaining) {
                    results.push(SearchResult {
                        sheet_index,
                        sheet_name: sheet_name.clone(),
                        row: cell.row,
                        col: cell.col,
                        value: cell.display_text,
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
        cells: Vec<SearchCellText>,
    },
    Scan {
        sheet_index: usize,
        sheet_name: String,
        cells: Vec<SearchCellText>,
    },
}

fn search_input_for_sheet(
    editor_state: &crate::state::editor_state::EditorState,
    sheet_index: usize,
    query: &str,
    matcher: Option<&SearchMatcher>,
    limit: usize,
) -> Option<SearchInput> {
    if let Some(cells) = editor_state.indexed_search_sheet(sheet_index, query, limit) {
        return Some(SearchInput::Indexed {
            sheet_index,
            sheet_name: editor_state.sheet_name(sheet_index)?,
            cells,
        });
    }
    let sheet = editor_state.file_data().sheets.get(sheet_index)?;
    matcher?;
    Some(SearchInput::Scan {
        sheet_index,
        sheet_name: sheet.name.clone(),
        cells: collect_sheet_search_text(sheet),
    })
}

fn scan_sheet(
    sheet_cells: &[SearchCellText],
    matcher: Option<&SearchMatcher>,
    limit: usize,
) -> Vec<SearchCellText> {
    let Some(matcher) = matcher else {
        return Vec::new();
    };
    let mut cells = Vec::new();
    for cell in sheet_cells {
        if !matcher.matches(&cell.search_text) {
            continue;
        }
        cells.push(cell.clone());
        if cells.len() >= limit {
            return cells;
        }
    }
    cells
}
