use crate::adapters::search_index_store::{SearchQueryPlan, SearchSheetIndex};
use crate::application::search_ports::SearchDocumentSourcePort;
use crate::domain::{SearchHit, SearchOutcome, SearchScanCursor, SearchScope};
use crate::editor_protocol::MAX_SEARCH_RESPONSE_BYTES;
use crate::error::AppError;

const SEARCH_RESULT_LIMIT: usize = 1000;
pub(crate) const MAX_SEARCH_RESULT_SNIPPET_BYTES: usize = 512;
const MAX_ON_DEMAND_INDEX_REBUILDS_PER_SEARCH: usize = 1;
const MAX_SEARCH_SCAN_CHUNK_TEXT_BYTES: usize = 8 * 1024 * 1024;
const MAX_SEARCH_SCAN_CHUNK_CELLS: usize = 32_768;

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
pub(crate) fn do_search<P>(
    source: &dyn SearchDocumentSourcePort,
    document_id: u64,
    base_revision: u64,
    query: &str,
    scope: SearchScope,
    current_sheet_index: Option<usize>,
    mut indexed_sheet: impl FnMut(usize) -> Option<std::sync::Arc<SearchSheetIndex>>,
    mut reserve_scan_work: impl FnMut() -> Result<P, AppError>,
    mut schedule_rebuild: impl FnMut(usize),
) -> Result<SearchOutcome, AppError> {
    let Some(plan) = SearchQueryPlan::try_new(query)? else {
        return Ok(SearchOutcome::default());
    };

    let document = source
        .document_snapshot(document_id, Some(base_revision))?
        .ok_or_else(|| {
            AppError::DocumentStateInvalid("search document is no longer active".to_string())
        })?;
    let sheet_indexes = match scope {
        SearchScope::CurrentSheet => vec![current_sheet_index.unwrap_or(0)],
        SearchScope::AllSheets => (0..document.sheets.len()).collect(),
    };

    let mut results = SearchResultCollector::new()?;
    let mut used_scan_fallback = false;
    let mut scan_reservation = None;
    let mut on_demand_rebuilds = Vec::new();

    for sheet_index in sheet_indexes {
        if results.is_truncated() || results.len() >= SEARCH_RESULT_LIMIT {
            results.mark_truncated();
            break;
        }
        let Some(sheet) = document.sheets.get(sheet_index) else {
            continue;
        };
        let input = SearchInput {
            sheet_index,
            sheet_name: sheet.name.clone(),
            index: indexed_sheet(sheet_index),
        };
        match input.index.as_ref() {
            Some(index) => {
                let remaining = SEARCH_RESULT_LIMIT - results.len();
                let cells = index.search(&plan, remaining);
                for cell in cells.into_iter().take(remaining) {
                    if !results.try_push(search_result(
                        &input,
                        &cell.display_text,
                        cell.row,
                        cell.col,
                        &plan,
                    ))? {
                        break;
                    }
                }
            }
            None => {
                used_scan_fallback = true;
                if scan_reservation.is_none() {
                    scan_reservation = Some(reserve_scan_work()?);
                }
                if on_demand_rebuilds.len() < MAX_ON_DEMAND_INDEX_REBUILDS_PER_SEARCH {
                    on_demand_rebuilds.push(input.sheet_index);
                }
                scan_sheet_fallback(
                    source,
                    document_id,
                    base_revision,
                    &input,
                    &plan,
                    &mut results,
                )?;
            }
        }
    }

    drop(scan_reservation);
    if used_scan_fallback {
        eprintln!("Search used bounded scan fallback while index was stale or unavailable");
    }
    for sheet_index in on_demand_rebuilds {
        schedule_rebuild(sheet_index);
    }

    source.document_snapshot(document_id, Some(base_revision))?;

    results.finish()
}

fn scan_sheet_fallback(
    source: &dyn SearchDocumentSourcePort,
    document_id: u64,
    base_revision: u64,
    input: &SearchInput,
    plan: &SearchQueryPlan,
    results: &mut SearchResultCollector,
) -> Result<(), AppError> {
    let mut cursor = SearchScanCursor::default();
    loop {
        let chunk = source.sheet_text_chunk(
            document_id,
            base_revision,
            input.sheet_index,
            cursor,
            MAX_SEARCH_SCAN_CHUNK_TEXT_BYTES,
            MAX_SEARCH_SCAN_CHUNK_CELLS,
        )?;
        let Some(chunk) = chunk else {
            return Ok(());
        };
        for cell in chunk.cells {
            if !plan.matches(&cell.search_text) {
                continue;
            }
            if !results.try_push(search_result(
                input,
                &cell.display_text,
                cell.row,
                cell.col,
                plan,
            ))? {
                return Ok(());
            }
        }
        let Some(next) = chunk.next else {
            return Ok(());
        };
        cursor = next;
    }
}

fn search_result(
    input: &SearchInput,
    display_text: &str,
    row: usize,
    col: usize,
    plan: &SearchQueryPlan,
) -> SearchHit {
    SearchHit {
        sheet_index: input.sheet_index,
        sheet_name: input.sheet_name.clone(),
        row,
        col,
        value: bounded_search_snippet(display_text, plan, MAX_SEARCH_RESULT_SNIPPET_BYTES),
        cell_position: format!("{}{}", col_to_letter(col), row + 1),
    }
}

struct SearchInput {
    sheet_index: usize,
    sheet_name: String,
    index: Option<std::sync::Arc<SearchSheetIndex>>,
}

struct SearchResultCollector {
    results: Vec<SearchHit>,
    estimated_bytes: usize,
    truncated: bool,
}

impl SearchResultCollector {
    fn new() -> Result<Self, AppError> {
        Ok(Self {
            results: Vec::new(),
            estimated_bytes: 64,
            truncated: false,
        })
    }

    fn len(&self) -> usize {
        self.results.len()
    }

    fn is_truncated(&self) -> bool {
        self.truncated
    }

    fn mark_truncated(&mut self) {
        self.truncated = true;
    }

    fn try_push(&mut self, result: SearchHit) -> Result<bool, AppError> {
        if self.results.len() >= SEARCH_RESULT_LIMIT {
            self.truncated = true;
            return Ok(false);
        }
        let result_bytes = estimated_search_hit_bytes(&result);
        let projected_bytes = self.estimated_bytes.saturating_add(result_bytes);
        if projected_bytes > MAX_SEARCH_RESPONSE_BYTES {
            self.truncated = true;
            return Ok(false);
        }
        self.results.push(result);
        self.estimated_bytes = projected_bytes;
        Ok(true)
    }

    fn finish(mut self) -> Result<SearchOutcome, AppError> {
        if self.results.len() >= SEARCH_RESULT_LIMIT {
            self.truncated = true;
        }
        Ok(SearchOutcome {
            results: self.results,
            truncated: self.truncated,
        })
    }
}

fn bounded_search_snippet(value: &str, plan: &SearchQueryPlan, maximum_bytes: usize) -> String {
    if value.len() <= maximum_bytes {
        return value.to_string();
    }
    if maximum_bytes <= 6 {
        return truncate_utf8(value, maximum_bytes).to_string();
    }

    let content_bytes = maximum_bytes - 6;
    let anchor = plan.first_match_byte(value).unwrap_or(0);
    let mut start = anchor.saturating_sub(content_bytes / 3);
    start = start.min(value.len().saturating_sub(content_bytes));
    while start < value.len() && !value.is_char_boundary(start) {
        start += 1;
    }
    let mut end = start.saturating_add(content_bytes).min(value.len());
    while end > start && !value.is_char_boundary(end) {
        end -= 1;
    }

    let mut snippet = String::with_capacity(maximum_bytes);
    if start > 0 {
        snippet.push_str("...");
    }
    snippet.push_str(&value[start..end]);
    if end < value.len() {
        snippet.push_str("...");
    }
    snippet
}

fn truncate_utf8(value: &str, maximum_bytes: usize) -> &str {
    if value.len() <= maximum_bytes {
        return value;
    }
    let mut end = maximum_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn estimated_search_hit_bytes(result: &SearchHit) -> usize {
    128usize
        .saturating_add(result.sheet_name.len().saturating_mul(6))
        .saturating_add(result.value.len().saturating_mul(6))
        .saturating_add(result.cell_position.len().saturating_mul(6))
}

#[cfg(test)]
mod query_limit_tests {
    use super::*;
    use crate::adapters::search_index_store::{MAX_SEARCH_QUERY_BYTES, MAX_SEARCH_QUERY_TERMS};

    #[test]
    fn search_query_rejects_oversized_text_before_accessing_the_document() {
        let error = SearchQueryPlan::try_new(&"x".repeat(MAX_SEARCH_QUERY_BYTES + 1))
            .expect_err("oversized search query");

        assert!(matches!(error, AppError::ResourceLimitExceeded(_)));
    }

    #[test]
    fn search_query_rejects_too_many_unique_terms() {
        let query = (0..=MAX_SEARCH_QUERY_TERMS)
            .map(|index| format!("term{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        let error = SearchQueryPlan::try_new(&query).expect_err("too many terms");

        assert!(matches!(error, AppError::ResourceLimitExceeded(_)));
    }

    #[test]
    fn search_snippet_is_utf8_bounded_and_keeps_the_match_visible() {
        let plan = SearchQueryPlan::try_new("needle")
            .expect("query plan")
            .expect("nonempty query");
        let value = format!("{}needle{}", "前".repeat(300), "后".repeat(300));

        let snippet = bounded_search_snippet(&value, &plan, MAX_SEARCH_RESULT_SNIPPET_BYTES);

        assert!(snippet.len() <= MAX_SEARCH_RESULT_SNIPPET_BYTES);
        assert!(snippet.contains("needle"));
        assert!(std::str::from_utf8(snippet.as_bytes()).is_ok());
    }

    #[test]
    fn search_result_collector_enforces_internal_memory_budget() {
        let mut collector = SearchResultCollector::new().expect("collector");
        for row in 0..SEARCH_RESULT_LIMIT {
            if !collector
                .try_push(SearchHit {
                    sheet_index: 0,
                    sheet_name: "Sheet1".to_string(),
                    row,
                    col: 0,
                    value: "\0".repeat(MAX_SEARCH_RESULT_SNIPPET_BYTES),
                    cell_position: format!("A{}", row + 1),
                })
                .expect("bounded result")
            {
                break;
            }
        }

        let response = collector.finish().expect("bounded response");
        assert!(response.truncated);
        assert!(response.results.len() < SEARCH_RESULT_LIMIT);
        assert!(collector_estimated_bytes(&response) <= MAX_SEARCH_RESPONSE_BYTES);
    }

    fn collector_estimated_bytes(response: &SearchOutcome) -> usize {
        64 + response
            .results
            .iter()
            .map(estimated_search_hit_bytes)
            .sum::<usize>()
    }
}
