use crate::io::rich_projection::{RichProjectionScope, filter_rich_projection};
use crate::state::editor_state::{EditorState, ExecutedOperation};
use crate::state::state::EditorStateInfo;
use crate::types::{
    AppliedOperationResult, ColumnDeletedPatch, ColumnInsertedPatch, EditorMutationResponse,
    EditorPatch, FileData, LayoutPatch, ResyncRequiredPatch, RichProjectionPatch,
    RichProjectionPatchScope, RowDeletedPatch, RowInsertedPatch, SearchIndexUpdatePlan,
    SheetCellChange, SheetDeletedPatch, SheetInsertedPatch, SheetStructureMetadataPatch,
    SheetUpdatedPatch,
};

const EDITOR_MUTATION_PROTOCOL_VERSION: u16 = 1;

pub fn editor_state_info(editor_state: &EditorState) -> EditorStateInfo {
    EditorStateInfo {
        can_undo: editor_state.can_undo(),
        can_redo: editor_state.can_redo(),
        is_dirty: editor_state.is_dirty(),
        history: editor_state.history_status(),
    }
}

pub fn mutation_response(
    editor_state: &EditorState,
    patches: Vec<EditorPatch>,
) -> EditorMutationResponse {
    mutation_response_with_search_index_update(
        editor_state,
        patches,
        SearchIndexUpdatePlan::default(),
    )
}

pub fn mutation_response_with_search_index_update(
    editor_state: &EditorState,
    patches: Vec<EditorPatch>,
    search_index_update: SearchIndexUpdatePlan,
) -> EditorMutationResponse {
    let patches = project_patch_display_formats(editor_state.file_data(), patches);
    EditorMutationResponse {
        protocol_version: EDITOR_MUTATION_PROTOCOL_VERSION,
        document_id: editor_state.document_id(),
        revision: editor_state.revision(),
        formula_status: editor_state.formula_status(),
        capabilities: editor_state.capabilities(),
        editor_state: editor_state_info(editor_state),
        patches,
        search_index_update,
    }
}

pub fn resync_required_mutation_response(
    editor_state: &EditorState,
    reason: impl Into<String>,
) -> EditorMutationResponse {
    mutation_response(
        editor_state,
        vec![EditorPatch::ResyncRequired {
            patch: ResyncRequiredPatch {
                reason: reason.into(),
            },
        }],
    )
}

pub fn status_mutation_response(editor_state: &EditorState) -> EditorMutationResponse {
    mutation_response(editor_state, Vec::new())
}

pub fn cell_delta_mutation_response(
    editor_state: &EditorState,
    operation: &AppliedOperationResult,
    mut cell_changes: Vec<SheetCellChange>,
) -> EditorMutationResponse {
    if let AppliedOperationResult::SetCell { sheet_index, cell } = operation {
        push_cell_change_if_missing(
            &mut cell_changes,
            SheetCellChange::new(*sheet_index, cell.row, cell.col, cell.value.clone()),
        );
    }
    if let AppliedOperationResult::SetCells { changes } = operation {
        for change in changes {
            push_cell_change_if_missing(&mut cell_changes, change.clone());
        }
    }

    mutation_response(
        editor_state,
        if cell_changes.is_empty() {
            Vec::new()
        } else {
            vec![EditorPatch::Cells {
                changes: cell_changes,
            }]
        },
    )
}

pub fn layout_mutation_response(
    editor_state: &EditorState,
    patch: LayoutPatch,
) -> EditorMutationResponse {
    mutation_response(editor_state, vec![EditorPatch::Layout { patch }])
}

pub fn structural_delta_mutation_response(
    editor_state: &EditorState,
    operation: &AppliedOperationResult,
    cell_changes: Vec<SheetCellChange>,
    search_index_update: SearchIndexUpdatePlan,
) -> EditorMutationResponse {
    let mut patches = structural_patches(editor_state.file_data(), operation);

    if !cell_changes.is_empty() {
        patches.push(EditorPatch::Cells {
            changes: cell_changes,
        });
    }

    mutation_response_with_search_index_update(editor_state, patches, search_index_update)
}

pub fn restore_mutation_response(
    editor_state: &EditorState,
    result: ExecutedOperation,
) -> EditorMutationResponse {
    let Some(restore) = result.restore else {
        return resync_required_mutation_response(
            editor_state,
            "restore completed without patch details",
        );
    };

    mutation_response_with_search_index_update(
        editor_state,
        restore.patches,
        result.search_index_update,
    )
}

pub fn structural_patches(
    file_data: &FileData,
    operation: &AppliedOperationResult,
) -> Vec<EditorPatch> {
    match operation {
        AppliedOperationResult::AddRow { sheet_index, row } => file_data
            .sheets
            .get(*sheet_index)
            .map(|sheet| {
                vec![EditorPatch::RowInserted {
                    patch: RowInsertedPatch {
                        sheet_index: *sheet_index,
                        row_index: row.index,
                        rows: vec![row.values.clone()],
                        metadata: row_structure_metadata_patch(sheet, row.index),
                    },
                }]
            })
            .unwrap_or_default(),
        AppliedOperationResult::DeleteRow {
            sheet_index,
            row_index,
        } => file_data
            .sheets
            .get(*sheet_index)
            .map(|sheet| {
                vec![EditorPatch::RowDeleted {
                    patch: RowDeletedPatch {
                        sheet_index: *sheet_index,
                        row_index: *row_index,
                        count: 1,
                        metadata: row_structure_metadata_patch(sheet, *row_index),
                    },
                }]
            })
            .unwrap_or_default(),
        AppliedOperationResult::AddColumn {
            sheet_index,
            column,
            col_data,
        } => file_data
            .sheets
            .get(*sheet_index)
            .map(|sheet| {
                vec![EditorPatch::ColumnInserted {
                    patch: ColumnInsertedPatch {
                        sheet_index: *sheet_index,
                        col_index: column.index,
                        values: col_data.clone(),
                        metadata: column_structure_metadata_patch(sheet, column.index),
                    },
                }]
            })
            .unwrap_or_default(),
        AppliedOperationResult::DeleteColumn {
            sheet_index,
            column_index,
        } => file_data
            .sheets
            .get(*sheet_index)
            .map(|sheet| {
                vec![EditorPatch::ColumnDeleted {
                    patch: ColumnDeletedPatch {
                        sheet_index: *sheet_index,
                        col_index: *column_index,
                        count: 1,
                        metadata: column_structure_metadata_patch(sheet, *column_index),
                    },
                }]
            })
            .unwrap_or_default(),
        AppliedOperationResult::AddSheet {
            sheet_index,
            sheet_data,
            ..
        } => vec![EditorPatch::SheetInserted {
            patch: SheetInsertedPatch {
                sheet_index: *sheet_index,
                sheet: sheet_data.clone(),
            },
        }],
        AppliedOperationResult::DeleteSheet { sheet_index, .. } => {
            vec![EditorPatch::SheetDeleted {
                patch: SheetDeletedPatch {
                    sheet_index: *sheet_index,
                },
            }]
        }
        AppliedOperationResult::SetCell { .. }
        | AppliedOperationResult::SetCells { .. }
        | AppliedOperationResult::SetColumnWidth { .. }
        | AppliedOperationResult::SetRowHeight { .. } => Vec::new(),
    }
}

pub(crate) fn sheet_updated_patch(file_data: &FileData, sheet_index: usize) -> Vec<EditorPatch> {
    file_data
        .sheets
        .get(sheet_index)
        .cloned()
        .map(|sheet| {
            vec![EditorPatch::SheetUpdated {
                patch: SheetUpdatedPatch { sheet_index, sheet },
            }]
        })
        .unwrap_or_default()
}

fn row_structure_metadata_patch(
    sheet: &crate::types::SheetData,
    start: usize,
) -> SheetStructureMetadataPatch {
    SheetStructureMetadataPatch {
        merges: sheet.merges.clone(),
        column_widths: sheet.column_widths.clone(),
        row_heights: sheet.row_heights.clone(),
        rich: RichProjectionPatch {
            scope: RichProjectionPatchScope::Rows { start },
            projection: filter_rich_projection(&sheet.rich, RichProjectionScope::Rows { start }),
        },
    }
}

fn column_structure_metadata_patch(
    sheet: &crate::types::SheetData,
    start: usize,
) -> SheetStructureMetadataPatch {
    SheetStructureMetadataPatch {
        merges: sheet.merges.clone(),
        column_widths: sheet.column_widths.clone(),
        row_heights: sheet.row_heights.clone(),
        rich: RichProjectionPatch {
            scope: RichProjectionPatchScope::Columns { start },
            projection: filter_rich_projection(&sheet.rich, RichProjectionScope::Columns { start }),
        },
    }
}

fn project_patch_display_formats(
    file_data: &FileData,
    patches: Vec<EditorPatch>,
) -> Vec<EditorPatch> {
    patches
        .into_iter()
        .map(|patch| match patch {
            EditorPatch::Cells { changes } => EditorPatch::Cells {
                changes: changes
                    .into_iter()
                    .map(|change| {
                        let sheet = file_data.sheets.get(change.sheet_index);
                        let display = sheet
                            .map(|sheet| sheet.cell_display_text(change.row, change.col))
                            .unwrap_or_else(|| change.value.to_display_string());
                        let format =
                            sheet.and_then(|sheet| sheet.cell_format_at(change.row, change.col));
                        let style =
                            sheet.and_then(|sheet| sheet.cell_style_at(change.row, change.col));
                        change.with_display_projection(display, format, style)
                    })
                    .collect(),
            },
            other => other,
        })
        .collect()
}

fn push_cell_change_if_missing(cell_changes: &mut Vec<SheetCellChange>, change: SheetCellChange) {
    if !cell_changes.iter().any(|existing| {
        existing.sheet_index == change.sheet_index
            && existing.row == change.row
            && existing.col == change.col
    }) {
        cell_changes.push(change);
    }
}
