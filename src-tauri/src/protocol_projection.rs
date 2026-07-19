use crate::document::capabilities as document_capabilities;
use crate::document::region_metadata_index::{DocumentRegion, DocumentRegionMetadata};
use crate::document_data::{CellFormat, CellStyle, MergeRange, SheetExtent};
use crate::domain::SearchOutcome;
use crate::editor_protocol::{
    EDITOR_MUTATION_PROTOCOL_VERSION, MAX_MUTATION_RESPONSE_BYTES, MAX_SEARCH_RESPONSE_BYTES,
    MAX_SHEET_REGION_RESPONSE_BYTES,
};
use crate::error::AppError;
use crate::formula::status as formula_status;
use crate::projection_model::{
    DocumentCapabilities, DocumentManifestSnapshot, EditorSessionSnapshot, EditorStateSnapshot,
    MutationLookup, MutationLookupStatus, MutationOutcome, MutationPatch, NativeSavePlan,
    OpenDocumentSnapshot, PreparedOpenDocument, ProjectedCellChange, SavedDocumentOutcome,
    SheetLayoutSnapshot, SheetManifestSnapshot, SheetRegionSnapshot, SpreadsheetFormatOptions,
};
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

pub(crate) fn prepared_open_document(value: PreparedOpenDocument) -> types::PreparedOpenDocument {
    types::PreparedOpenDocument { token: value.token }
}

pub(crate) fn editor_session(value: EditorSessionSnapshot) -> types::EditorSessionInfo {
    types::EditorSessionInfo {
        document_id: value.document_id,
        revision: value.revision,
        formula_status: formula_status(value.formula_status, 100),
        capabilities: workbook_capabilities(value.capabilities),
        editor_state: editor_state(value.editor_state),
    }
}

fn editor_state(value: EditorStateSnapshot) -> types::EditorStateInfo {
    types::EditorStateInfo {
        can_undo: value.can_undo,
        can_redo: value.can_redo,
        is_dirty: value.is_dirty,
        history: history_status(value.history),
    }
}

pub(crate) fn open_document_response(value: OpenDocumentSnapshot) -> types::OpenDocumentResponse {
    types::OpenDocumentResponse {
        document: document_manifest(value.document),
        editor_session: editor_session(value.editor_session),
        initial_region: value
            .initial_region
            .and_then(|region| sheet_region_response(region, MAX_SHEET_REGION_RESPONSE_BYTES).ok()),
    }
}

pub(crate) fn saved_document_response(value: SavedDocumentOutcome) -> types::SavedDocumentResponse {
    types::SavedDocumentResponse {
        document: value.document.map(document_manifest),
        identity: value.identity.map(|identity| types::SavedDocumentIdentity {
            path: identity.path,
            file_name: identity.file_name,
        }),
        editor_session: editor_session(value.editor_session),
    }
}

pub(crate) fn sheet_region_response(
    value: SheetRegionSnapshot,
    maximum_bytes: usize,
) -> Result<types::SheetRegionProjectionResponse, AppError> {
    let mut response = types::SheetRegionProjectionResponse {
        document_id: value.document_id,
        revision: value.revision,
        region: sheet_region(value.region),
        cells: value.cells.into_iter().map(projected_cell_change).collect(),
        merge_anchor_cells: value
            .merge_anchor_cells
            .into_iter()
            .map(projected_cell_change)
            .collect(),
        metadata: region_metadata(value.metadata),
        estimated_bytes: None,
    };
    let mut estimate = serialized_json_bytes(&response)?;
    for _ in 0..8 {
        response.estimated_bytes = Some(estimate);
        let actual = serialized_json_bytes(&response)?;
        if actual == estimate {
            if actual > maximum_bytes {
                return Err(AppError::RegionResponseTooLarge {
                    estimated_bytes: actual,
                    maximum_bytes,
                });
            }
            return Ok(response);
        }
        estimate = actual;
    }
    Err(AppError::Internal(
        "failed to converge while sizing region response".to_string(),
    ))
}

pub(crate) fn document_capabilities(value: DocumentCapabilities) -> types::DocumentCapabilities {
    types::DocumentCapabilities {
        source_format: value.source_format,
        can_save_original: value.can_save_original,
        native_save_format: value.native_save_format,
        export_formats: value.export_formats,
        native_save_extension: value.native_save_extension,
        export_extension: value.export_extension,
        requires_save_as_for_native_save: value.requires_save_as_for_native_save,
        workbook: workbook_capabilities(value.workbook),
    }
}

pub(crate) fn native_save_plan(value: NativeSavePlan) -> types::NativeSavePlan {
    types::NativeSavePlan {
        can_save: value.can_save,
        requires_save_as: value.requires_save_as,
        native_save_extension: value.native_save_extension,
        default_extension: value.default_extension,
        blocked_reason: value.blocked_reason,
        capabilities: document_capabilities(value.capabilities),
    }
}

pub(crate) fn spreadsheet_format_options(
    value: SpreadsheetFormatOptions,
) -> types::SpreadsheetFormatOptions {
    types::SpreadsheetFormatOptions {
        default_extension: value.default_extension,
        supported_extensions: value.supported_extensions,
    }
}

pub(crate) fn mutation_response(value: MutationOutcome) -> types::EditorMutationResponse {
    let mut response = types::EditorMutationResponse {
        protocol_version: EDITOR_MUTATION_PROTOCOL_VERSION,
        document_id: value.document_id,
        revision: value.revision,
        formula_status: formula_status(value.session.formula_status, 100),
        capabilities: workbook_capabilities(value.session.capabilities),
        editor_state: editor_state(value.session.editor_state),
        patches: value.patches.into_iter().map(mutation_patch).collect(),
        sheet_extents: value
            .sheet_extents
            .map(|extents| extents.into_iter().map(sheet_extent).collect()),
    };
    if serialized_json_bytes(&response).is_ok_and(|bytes| bytes <= MAX_MUTATION_RESPONSE_BYTES) {
        return response;
    }
    response.patches = vec![types::EditorPatch::ResyncRequired {
        patch: types::ResyncRequiredPatch {
            reason: "mutation response exceeded the response byte limit".to_string(),
        },
    }];
    response
}

pub(crate) fn mutation_lookup(value: MutationLookup) -> types::MutationResultLookup {
    types::MutationResultLookup {
        status: match value.status {
            MutationLookupStatus::Pending => types::MutationResultStatus::Pending,
            MutationLookupStatus::Completed => types::MutationResultStatus::Completed,
            MutationLookupStatus::Missing => types::MutationResultStatus::Missing,
        },
        response: value.response.map(mutation_response),
    }
}

fn document_manifest(value: DocumentManifestSnapshot) -> types::DocumentManifest {
    types::DocumentManifest {
        path: value.path,
        file_name: value.file_name,
        sheets: value.sheets.into_iter().map(sheet_manifest).collect(),
    }
}

fn sheet_manifest(value: SheetManifestSnapshot) -> types::SheetManifest {
    types::SheetManifest {
        name: value.name,
        extent: sheet_extent(value.extent),
        layout: sheet_layout(value.layout),
    }
}

fn sheet_extent(value: SheetExtent) -> types::SheetExtent {
    types::SheetExtent {
        row_count: value.row_count,
        column_count: value.column_count,
    }
}

fn sheet_layout(value: SheetLayoutSnapshot) -> types::SheetLayoutProjection {
    types::SheetLayoutProjection {
        column_widths: value.column_widths,
        row_heights: value.row_heights,
    }
}

fn sheet_region(value: DocumentRegion) -> types::SheetRegion {
    types::SheetRegion {
        sheet_index: value.sheet_index,
        row_start: value.row_start,
        row_end: value.row_end,
        col_start: value.col_start,
        col_end: value.col_end,
    }
}

fn projected_cell_change(value: ProjectedCellChange) -> types::SheetCellChange {
    let format = value.format.map(cell_format);
    let style = value.style.map(cell_style);
    let display = value
        .display
        .unwrap_or_else(|| value.value.to_display_string());
    types::SheetCellChange::new(value.sheet_index, value.row, value.col, value.value)
        .with_display_projection(display, format, style)
}

fn mutation_patch(value: MutationPatch) -> types::EditorPatch {
    match value {
        MutationPatch::Cells { changes } => types::EditorPatch::Cells {
            changes: changes.into_iter().map(projected_cell_change).collect(),
        },
        MutationPatch::Layout {
            sheet_index,
            column_widths,
            row_heights,
        } => types::EditorPatch::Layout {
            patch: types::LayoutPatch {
                sheet_index,
                column_widths,
                row_heights,
            },
        },
        MutationPatch::SheetInserted { sheet_index, sheet } => types::EditorPatch::SheetInserted {
            patch: types::SheetInsertedPatch {
                sheet_index,
                sheet: sheet_manifest(sheet),
            },
        },
        MutationPatch::SheetDeleted { sheet_index } => types::EditorPatch::SheetDeleted {
            patch: types::SheetDeletedPatch { sheet_index },
        },
        MutationPatch::SheetInvalidated { sheet_index } => types::EditorPatch::SheetInvalidated {
            patch: types::SheetInvalidatedPatch { sheet_index },
        },
        MutationPatch::SheetsReplaced {
            start_index,
            sheets,
        } => types::EditorPatch::SheetsReplaced {
            patch: types::SheetsReplacedPatch {
                start_index,
                sheets: sheets.into_iter().map(sheet_manifest).collect(),
            },
        },
        MutationPatch::RowInserted {
            sheet_index,
            row_index,
            count,
        } => types::EditorPatch::RowInserted {
            patch: types::RowInsertedPatch {
                sheet_index,
                row_index,
                count,
            },
        },
        MutationPatch::RowDeleted {
            sheet_index,
            row_index,
            count,
        } => types::EditorPatch::RowDeleted {
            patch: types::RowDeletedPatch {
                sheet_index,
                row_index,
                count,
            },
        },
        MutationPatch::ColumnInserted {
            sheet_index,
            col_index,
            count,
        } => types::EditorPatch::ColumnInserted {
            patch: types::ColumnInsertedPatch {
                sheet_index,
                col_index,
                count,
            },
        },
        MutationPatch::ColumnDeleted {
            sheet_index,
            col_index,
            count,
        } => types::EditorPatch::ColumnDeleted {
            patch: types::ColumnDeletedPatch {
                sheet_index,
                col_index,
                count,
            },
        },
        MutationPatch::ResyncRequired { reason } => types::EditorPatch::ResyncRequired {
            patch: types::ResyncRequiredPatch { reason },
        },
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
