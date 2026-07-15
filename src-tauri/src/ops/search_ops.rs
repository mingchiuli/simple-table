use std::sync::{Arc, RwLock};

use crate::error::AppError;
use crate::state::search_index::{SearchCellText, SearchQueryPlan, collect_sheet_search_text};
use crate::state::search_service::SearchService;
use crate::state::state::ActiveDocumentStore;
use crate::types::{SearchResult, SearchScope};

const SEARCH_RESULT_LIMIT: usize = 1000;
const MAX_ON_DEMAND_INDEX_REBUILDS_PER_SEARCH: usize = 1;

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

    let sheet_indexes = {
        let registry = registry
            .read()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        let editor_state = registry.active_for_command(document_id, base_revision)?;

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

    let mut results = Vec::new();
    let mut used_scan_fallback = false;
    let mut on_demand_rebuilds = Vec::new();

    for sheet_index in sheet_indexes {
        if results.len() >= SEARCH_RESULT_LIMIT {
            break;
        }
        let input = {
            let registry = registry
                .read()
                .map_err(|_| AppError::poisoned_lock("document registry"))?;
            let editor_state = registry.active_for_command(document_id, base_revision)?;
            let Some(sheet_name) = editor_state.sheet_name(sheet_index) else {
                continue;
            };
            let index = editor_state.indexed_search_sheet(sheet_index);
            let sheet = index
                .is_none()
                .then(|| editor_state.search_sheet_data(sheet_index))
                .flatten();
            SearchInput {
                sheet_index,
                sheet_name,
                index,
                sheet,
            }
        };
        let remaining = SEARCH_RESULT_LIMIT - results.len();
        match input.index {
            Some(index) => {
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
            None => {
                let Some(sheet) = input.sheet else { continue };
                used_scan_fallback = true;
                if on_demand_rebuilds.len() < MAX_ON_DEMAND_INDEX_REBUILDS_PER_SEARCH {
                    on_demand_rebuilds.push(input.sheet_index);
                }
                let cells = collect_sheet_search_text(&sheet);
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
    let search_service = SearchService::global();
    for sheet_index in on_demand_rebuilds {
        search_service.rebuild_sheet_index(registry, document_id, sheet_index);
    }

    Ok(results)
}

struct SearchInput {
    sheet_index: usize,
    sheet_name: String,
    index: Option<std::sync::Arc<crate::state::search_index::SearchSheetIndex>>,
    sheet: Option<crate::types::SheetData>,
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
