use crate::document::capabilities as document_capabilities;
use crate::document::region_metadata_index::DocumentRegionMetadata;
use crate::document_data::{CellFormat, CellStyle, MergeRange};
use crate::domain::SearchOutcome;
use crate::editor_protocol::MAX_SEARCH_RESPONSE_BYTES;
use crate::error::AppError;
use crate::formula::status as formula_status;
use crate::state::history_store::HistoryStatus as StateHistoryStatus;
use crate::types;
use std::io::Write;

pub(crate) fn formula_status(
    value: formula_status::FormulaStatus,
    maximum_issues: usize,
) -> types::FormulaStatus {
    match value {
        formula_status::FormulaStatus::Ready { diagnostics } => types::FormulaStatus::Ready {
            diagnostics: formula_diagnostics(diagnostics, maximum_issues),
        },
        formula_status::FormulaStatus::Degraded {
            message,
            diagnostics,
        } => types::FormulaStatus::Degraded {
            message,
            diagnostics: formula_diagnostics(diagnostics, maximum_issues),
        },
    }
}

fn formula_diagnostics(
    value: formula_status::FormulaDiagnostics,
    maximum_issues: usize,
) -> types::FormulaDiagnostics {
    types::FormulaDiagnostics {
        invalid_formula_count: value.invalid_formula_count,
        volatile_formula_count: value.volatile_formula_count,
        unsupported_dependency_count: value.unsupported_dependency_count,
        large_range_dependency_count: value.large_range_dependency_count,
        skipped_reference_rewrite_count: value.skipped_reference_rewrite_count,
        issues: value
            .issues
            .into_iter()
            .take(maximum_issues)
            .map(|issue| types::FormulaIssue {
                sheet_index: issue.sheet_index,
                row: issue.row,
                col: issue.col,
                kind: match issue.kind {
                    formula_status::FormulaIssueKind::InvalidFormula => {
                        types::FormulaIssueKind::InvalidFormula
                    }
                    formula_status::FormulaIssueKind::VolatileFormula => {
                        types::FormulaIssueKind::VolatileFormula
                    }
                    formula_status::FormulaIssueKind::UnsupportedDependency => {
                        types::FormulaIssueKind::UnsupportedDependency
                    }
                    formula_status::FormulaIssueKind::LargeRangeDependency => {
                        types::FormulaIssueKind::LargeRangeDependency
                    }
                },
                message: issue.message,
            })
            .collect(),
    }
}

pub(crate) fn workbook_capabilities(
    value: document_capabilities::WorkbookCapabilities,
) -> types::WorkbookCapabilities {
    types::WorkbookCapabilities {
        save: types::WorkbookSaveCapabilities {
            can_native_save: value.save.can_native_save,
            blocked_save_reasons: value.save.blocked_save_reasons,
            detected_features: value.save.detected_features,
        },
        structure: types::WorkbookStructureCapabilities {
            can_insert_delete_sheets: value.structure.can_insert_delete_sheets,
            blocked_structure_reasons: value.structure.blocked_structure_reasons,
            blocked_sheet_structure_reasons: value.structure.blocked_sheet_structure_reasons,
        },
        rich: types::WorkbookRichCapabilities {
            can_edit_styles: value.rich.can_edit_styles,
            can_edit_drawings: value.rich.can_edit_drawings,
            can_edit_hyperlinks: value.rich.can_edit_hyperlinks,
        },
        sheets: value
            .sheets
            .into_iter()
            .map(|sheet| types::SheetCapabilities {
                can_edit_cells: sheet.can_edit_cells,
                can_resize_rows_columns: sheet.can_resize_rows_columns,
                can_insert_delete_rows: sheet.can_insert_delete_rows,
                can_insert_delete_columns: sheet.can_insert_delete_columns,
                blocked_edit_reasons: sheet.blocked_edit_reasons,
                blocked_resize_reasons: sheet.blocked_resize_reasons,
                blocked_row_structure_reasons: sheet.blocked_row_structure_reasons,
                blocked_column_structure_reasons: sheet.blocked_column_structure_reasons,
            })
            .collect(),
    }
}

pub(crate) fn history_status(value: StateHistoryStatus) -> types::HistoryStatus {
    types::HistoryStatus {
        is_truncated: value.is_truncated,
        reason: value.reason,
        undo_entries: value.undo_entries,
        redo_entries: value.redo_entries,
        undo_estimated_bytes: value.undo_estimated_bytes,
        redo_estimated_bytes: value.redo_estimated_bytes,
        max_history_bytes: value.max_history_bytes,
        max_single_entry_bytes: value.max_single_entry_bytes,
    }
}

pub(crate) fn region_metadata(value: DocumentRegionMetadata) -> types::SheetRegionMetadata {
    types::SheetRegionMetadata {
        merges: value.merges.into_iter().map(merge_range).collect(),
        cell_formats: value
            .cell_formats
            .into_iter()
            .map(|(key, value)| (key, cell_format(value)))
            .collect(),
        cell_styles: value
            .cell_styles
            .into_iter()
            .map(|(key, value)| (key, cell_style(value)))
            .collect(),
    }
}

fn merge_range(value: MergeRange) -> types::MergeRange {
    types::MergeRange {
        start_row: value.start_row,
        start_col: value.start_col,
        end_row: value.end_row,
        end_col: value.end_col,
    }
}

fn cell_format(value: CellFormat) -> types::CellFormatProjection {
    types::CellFormatProjection {
        number_format: value.number_format,
        style_id: value.style_id,
    }
}

fn cell_style(value: CellStyle) -> types::CellStyleProjection {
    types::CellStyleProjection {
        font_color: value.font_color,
        background_color: value.background_color,
        bold: value.bold,
        italic: value.italic,
        horizontal_align: value.horizontal_align,
        vertical_align: value.vertical_align,
        number_format: value.number_format,
    }
}

pub(crate) fn search_response(value: SearchOutcome) -> Result<types::SearchResponse, AppError> {
    let mut response = types::SearchResponse {
        results: value
            .results
            .into_iter()
            .map(|result| types::SearchResult {
                sheet_index: result.sheet_index,
                sheet_name: result.sheet_name,
                row: result.row,
                col: result.col,
                value: result.value,
                cell_position: result.cell_position,
            })
            .collect(),
        truncated: value.truncated,
    };

    if serialized_search_response_bytes(&response.results, response.truncated)?
        > MAX_SEARCH_RESPONSE_BYTES
    {
        let mut admitted = 0usize;
        let mut rejected = response.results.len();
        while admitted < rejected {
            let candidate = admitted + (rejected - admitted).div_ceil(2);
            if serialized_search_response_bytes(&response.results[..candidate], true)?
                <= MAX_SEARCH_RESPONSE_BYTES
            {
                admitted = candidate;
            } else {
                rejected = candidate - 1;
            }
        }
        response.results.truncate(admitted);
        response.truncated = true;
    }
    if serialized_json_bytes(&response)? > MAX_SEARCH_RESPONSE_BYTES {
        return Err(AppError::Internal(
            "bounded search response exceeds its transport budget".to_string(),
        ));
    }
    Ok(response)
}

fn serialized_search_response_bytes(
    results: &[types::SearchResult],
    truncated: bool,
) -> Result<usize, AppError> {
    #[derive(serde::Serialize)]
    struct SearchResponseRef<'a> {
        results: &'a [types::SearchResult],
        truncated: bool,
    }

    serialized_json_bytes(&SearchResponseRef { results, truncated })
}

fn serialized_json_bytes(value: &impl serde::Serialize) -> Result<usize, AppError> {
    let mut counter = CountingWriter::default();
    serde_json::to_writer(&mut counter, value).map_err(|error| {
        AppError::Internal(format!("failed to size protocol response: {error}"))
    })?;
    Ok(counter.bytes)
}

#[derive(Default)]
struct CountingWriter {
    bytes: usize,
}

impl Write for CountingWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.bytes = self.bytes.saturating_add(buffer.len());
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::SearchHit;

    #[test]
    fn search_projection_enforces_the_serialized_response_budget() {
        let outcome = SearchOutcome {
            results: (0..1_000)
                .map(|row| SearchHit {
                    sheet_index: 0,
                    sheet_name: "Sheet1".to_string(),
                    row,
                    col: 0,
                    value: "\0".repeat(512),
                    cell_position: format!("A{}", row + 1),
                })
                .collect(),
            truncated: false,
        };

        let response = search_response(outcome).expect("bounded search response");

        assert!(response.truncated);
        assert!(serialized_json_bytes(&response).unwrap() <= MAX_SEARCH_RESPONSE_BYTES);
    }
}
