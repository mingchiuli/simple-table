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
            images: types::WorkbookImageCapabilities {
                can_insert: value.rich.images.can_insert,
                can_move_resize: value.rich.images.can_move_resize,
                can_delete: value.rich.images.can_delete,
                blocked_reasons: value.rich.images.blocked_reasons,
            },
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

pub(crate) fn sheet_image(value: crate::document_data::SheetImage) -> types::SheetImage {
    types::SheetImage {
        id: value.id,
        media_id: value.media_id,
        mime_type: value.mime_type,
        intrinsic_width: value.intrinsic_width,
        intrinsic_height: value.intrinsic_height,
        anchor: image_anchor(value.anchor),
        z_index: value.z_index,
        renderable: value.renderable,
    }
}

fn image_anchor(value: crate::document_data::ImageAnchor) -> types::ImageAnchor {
    match value {
        crate::document_data::ImageAnchor::OneCell {
            from,
            width_emu,
            height_emu,
        } => types::ImageAnchor::OneCell {
            from: image_marker(from),
            width_emu: u32::try_from(width_emu).unwrap_or(u32::MAX),
            height_emu: u32::try_from(height_emu).unwrap_or(u32::MAX),
        },
        crate::document_data::ImageAnchor::TwoCell { from, to } => types::ImageAnchor::TwoCell {
            from: image_marker(from),
            to: image_marker(to),
        },
    }
}

fn image_marker(value: crate::document_data::ImageMarker) -> types::ImageMarker {
    types::ImageMarker {
        row: value.row,
        col: value.col,
        row_offset_emu: value.row_offset_emu,
        col_offset_emu: value.col_offset_emu,
    }
}
