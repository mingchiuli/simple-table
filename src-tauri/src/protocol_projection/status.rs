use crate::document::capabilities as document_capabilities;
use crate::formula::status as formula_status;
use crate::state::history_store::HistoryStatus as StateHistoryStatus;
use crate::types;

pub(crate) fn editor_session(
    value: crate::projection_model::EditorSessionSnapshot,
) -> types::EditorSessionInfo {
    types::EditorSessionInfo {
        document_id: value.document_id,
        revision: value.revision,
        formula_status: formula_status(value.formula_status, 100),
        capabilities: workbook_capabilities(value.capabilities),
        editor_state: editor_state(value.editor_state),
    }
}

pub(super) fn formula_status(
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

pub(super) fn workbook_capabilities(
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

fn history_status(value: StateHistoryStatus) -> types::HistoryStatus {
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

pub(super) fn editor_state(
    value: crate::projection_model::EditorStateSnapshot,
) -> types::EditorStateInfo {
    types::EditorStateInfo {
        can_undo: value.can_undo,
        can_redo: value.can_redo,
        is_dirty: value.is_dirty,
        history: history_status(value.history),
    }
}
