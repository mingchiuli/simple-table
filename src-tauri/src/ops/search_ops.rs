use std::sync::{Arc, RwLock};

use crate::error::AppError;
use crate::state::search_index::{SearchCellText, SearchQueryPlan, SearchSheetSource};
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
    document_id: u64,
    base_revision: u64,
    query: &str,
    scope: SearchScope,
    current_sheet_index: Option<usize>,
) -> Result<Vec<SearchResult>, AppError> {
    let Some(plan) = SearchQueryPlan::new(query) else {
        return Ok(vec![]);
    };

    let inputs = {
        let registry = registry
            .read()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        let editor_state = registry.active_for_command(document_id, base_revision)?;

        let sheet_indexes = match scope {
            SearchScope::CurrentSheet => vec![current_sheet_index.unwrap_or(0)],
            SearchScope::AllSheets => editor_state
                .file_data()
                .sheets
                .iter()
                .enumerate()
                .map(|(sheet_idx, _)| sheet_idx)
                .collect(),
        };
        sheet_indexes
            .into_iter()
            .filter_map(|sheet_index| {
                Some(SearchInput {
                    sheet_index,
                    sheet_name: editor_state.sheet_name(sheet_index)?,
                    source: editor_state.search_sheet_source(sheet_index)?,
                })
            })
            .collect::<Vec<_>>()
    };

    let mut results = Vec::new();
    let mut used_scan_fallback = false;

    for input in inputs {
        if results.len() >= SEARCH_RESULT_LIMIT {
            break;
        }
        let remaining = SEARCH_RESULT_LIMIT - results.len();
        match input.source {
            SearchSheetSource::Indexed(index) => {
                let cells = index.search(&plan, remaining);
                for cell in cells.into_iter().take(remaining) {
                    results.push(SearchResult {
                        sheet_index: input.sheet_index,
                        sheet_name: input.sheet_name.clone(),
                        row: cell.row,
                        col: cell.col,
                        value: cell.display_text,
                        cell_position: format!("{}{}", col_to_letter(cell.col), cell.row + 1),
                    });
                }
            }
            SearchSheetSource::Snapshot(snapshot) => {
                used_scan_fallback = true;
                let cells = snapshot.materialize();
                for cell in scan_sheet(&cells, &plan, remaining) {
                    results.push(SearchResult {
                        sheet_index: input.sheet_index,
                        sheet_name: input.sheet_name.clone(),
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

struct SearchInput {
    sheet_index: usize,
    sheet_name: String,
    source: SearchSheetSource,
}

fn scan_sheet(
    sheet_cells: &[SearchCellText],
    plan: &SearchQueryPlan,
    limit: usize,
) -> Vec<SearchCellText> {
    let mut cells = Vec::new();
    for cell in sheet_cells {
        if !plan.matches(&cell.search_text) {
            continue;
        }
        cells.push(cell.clone());
        if cells.len() >= limit {
            return cells;
        }
    }
    cells
}
