use std::collections::{BTreeSet, HashMap};

use crate::document::document_restore::{DocumentRestoreChange, DocumentRestoreResult};
use crate::document_data::{DocumentData, SheetExtent};
use crate::domain::{CellValue, DocumentCellChange};
use crate::ops::operation_projection::ProjectedOperation;
use crate::projection_model::{
    EditorSessionSnapshot, EditorStateSnapshot, MutationOutcome, MutationPatch,
    ProjectedCellChange, SheetLayoutSnapshot, SheetManifestSnapshot,
};
use crate::state::editor_state::EditorState;

const MAX_CELL_CHANGES_PER_RESPONSE: usize = 4_096;
const MAX_CELL_PATCH_BYTES_PER_RESPONSE: usize = 2 * 1024 * 1024;

pub fn editor_state_snapshot(editor_state: &EditorState) -> EditorStateSnapshot {
    EditorStateSnapshot {
        can_undo: editor_state.can_undo(),
        can_redo: editor_state.can_redo(),
        is_dirty: editor_state.is_dirty(),
        history: editor_state.history_status(),
    }
}

pub fn mutation_outcome(
    editor_state: &EditorState,
    patches: Vec<MutationPatch>,
) -> MutationOutcome {
    let patches = bounded_patches(
        editor_state.file_data(),
        project_patch_display_formats(editor_state.file_data(), patches),
    );
    MutationOutcome {
        document_id: editor_state.document_id(),
        revision: editor_state.revision(),
        session: EditorSessionSnapshot {
            document_id: editor_state.document_id(),
            revision: editor_state.revision(),
            formula_status: editor_state.formula_status(),
            capabilities: editor_state.capabilities(),
            editor_state: editor_state_snapshot(editor_state),
        },
        patches,
        sheet_extents: Some(editor_state.sheet_extents()),
    }
}

pub fn resync_required_mutation_outcome(
    editor_state: &EditorState,
    reason: impl Into<String>,
) -> MutationOutcome {
    mutation_outcome(
        editor_state,
        vec![MutationPatch::ResyncRequired {
            reason: reason.into(),
        }],
    )
}

pub fn status_mutation_outcome(editor_state: &EditorState) -> MutationOutcome {
    mutation_outcome(editor_state, Vec::new())
}

pub fn cell_delta_mutation_outcome(
    editor_state: &EditorState,
    cell_changes: Vec<ProjectedCellChange>,
) -> MutationOutcome {
    mutation_outcome(
        editor_state,
        if cell_changes.is_empty() {
            Vec::new()
        } else {
            vec![MutationPatch::Cells {
                changes: cell_changes,
            }]
        },
    )
}

pub(crate) fn complete_cell_changes(
    operation: &ProjectedOperation,
    cell_changes: Vec<DocumentCellChange>,
) -> Vec<ProjectedCellChange> {
    let mut cell_changes = projected_cell_changes(cell_changes);
    if let ProjectedOperation::SetCell {
        sheet_index,
        row,
        col,
        value,
    } = operation
    {
        push_cell_change_if_missing(
            &mut cell_changes,
            ProjectedCellChange::new(*sheet_index, *row, *col, value.clone()),
        );
    }
    if let ProjectedOperation::SetCells { changes } = operation {
        for change in changes {
            push_cell_change_if_missing(&mut cell_changes, change.clone());
        }
    }
    cell_changes
}

pub fn layout_mutation_outcome(
    editor_state: &EditorState,
    sheet_index: usize,
    column_widths: HashMap<usize, Option<u32>>,
    row_heights: HashMap<usize, Option<u32>>,
) -> MutationOutcome {
    mutation_outcome(
        editor_state,
        vec![MutationPatch::Layout {
            sheet_index,
            column_widths,
            row_heights,
        }],
    )
}

pub fn structural_delta_mutation_outcome(
    editor_state: &EditorState,
    operation: &ProjectedOperation,
    cell_changes: Vec<DocumentCellChange>,
) -> MutationOutcome {
    let cell_changes = projected_cell_changes(cell_changes);
    let mut patches = structural_patches(editor_state.file_data(), operation);

    if !cell_changes.is_empty() {
        patches.push(MutationPatch::Cells {
            changes: cell_changes,
        });
    }

    mutation_outcome(editor_state, patches)
}

pub fn restore_mutation_outcome(
    editor_state: &EditorState,
    restore: Option<DocumentRestoreResult>,
) -> MutationOutcome {
    let Some(restore) = restore else {
        return resync_required_mutation_outcome(
            editor_state,
            "restore completed without patch details",
        );
    };

    mutation_outcome(
        editor_state,
        restore
            .changes
            .into_iter()
            .map(restore_change_patch)
            .collect(),
    )
}

fn restore_change_patch(change: DocumentRestoreChange) -> MutationPatch {
    match change {
        DocumentRestoreChange::Cells(changes) => MutationPatch::Cells {
            changes: projected_cell_changes(changes),
        },
        DocumentRestoreChange::Layout {
            sheet_index,
            column_widths,
            row_heights,
        } => MutationPatch::Layout {
            sheet_index,
            column_widths,
            row_heights,
        },
        DocumentRestoreChange::RowInserted {
            sheet_index,
            row_index,
            count,
        } => MutationPatch::RowInserted {
            sheet_index,
            row_index,
            count,
        },
        DocumentRestoreChange::RowDeleted {
            sheet_index,
            row_index,
            count,
        } => MutationPatch::RowDeleted {
            sheet_index,
            row_index,
            count,
        },
        DocumentRestoreChange::ColumnInserted {
            sheet_index,
            col_index,
            count,
        } => MutationPatch::ColumnInserted {
            sheet_index,
            col_index,
            count,
        },
        DocumentRestoreChange::ColumnDeleted {
            sheet_index,
            col_index,
            count,
        } => MutationPatch::ColumnDeleted {
            sheet_index,
            col_index,
            count,
        },
        DocumentRestoreChange::SheetsReplaced {
            start_index,
            sheets,
        } => MutationPatch::SheetsReplaced {
            start_index,
            sheets: sheets
                .into_iter()
                .map(|sheet| SheetManifestSnapshot {
                    name: sheet.name,
                    extent: SheetExtent {
                        row_count: sheet.row_count,
                        column_count: sheet.column_count,
                    },
                    layout: SheetLayoutSnapshot {
                        column_widths: sheet.column_widths,
                        row_heights: sheet.row_heights,
                    },
                })
                .collect(),
        },
        DocumentRestoreChange::SheetInvalidated { sheet_index } => {
            MutationPatch::SheetInvalidated { sheet_index }
        }
        DocumentRestoreChange::ResyncRequired { reason } => {
            MutationPatch::ResyncRequired { reason }
        }
    }
}

pub fn structural_patches(
    _file_data: &DocumentData,
    operation: &ProjectedOperation,
) -> Vec<MutationPatch> {
    match operation {
        ProjectedOperation::AddRow {
            sheet_index,
            row_index,
        } => vec![MutationPatch::RowInserted {
            sheet_index: *sheet_index,
            row_index: *row_index,
            count: 1,
        }],
        ProjectedOperation::DeleteRow {
            sheet_index,
            row_index,
        } => vec![MutationPatch::RowDeleted {
            sheet_index: *sheet_index,
            row_index: *row_index,
            count: 1,
        }],
        ProjectedOperation::AddColumn {
            sheet_index,
            col_index,
        } => vec![MutationPatch::ColumnInserted {
            sheet_index: *sheet_index,
            col_index: *col_index,
            count: 1,
        }],
        ProjectedOperation::DeleteColumn {
            sheet_index,
            column_index,
        } => vec![MutationPatch::ColumnDeleted {
            sheet_index: *sheet_index,
            col_index: *column_index,
            count: 1,
        }],
        ProjectedOperation::AddSheet { sheet_index, sheet } => {
            vec![MutationPatch::SheetInserted {
                sheet_index: *sheet_index,
                sheet: sheet.clone(),
            }]
        }
        ProjectedOperation::DeleteSheet { sheet_index } => {
            vec![MutationPatch::SheetDeleted {
                sheet_index: *sheet_index,
            }]
        }
        ProjectedOperation::SetCell { .. }
        | ProjectedOperation::SetCells { .. }
        | ProjectedOperation::SetColumnWidth
        | ProjectedOperation::SetRowHeight => Vec::new(),
    }
}

fn bounded_patches(file_data: &DocumentData, patches: Vec<MutationPatch>) -> Vec<MutationPatch> {
    let mut cell_change_count = 0usize;
    let mut estimated_bytes = 0usize;
    for patch in &patches {
        if let MutationPatch::Cells { changes } = patch {
            cell_change_count = cell_change_count.saturating_add(changes.len());
            for change in changes {
                estimated_bytes =
                    estimated_bytes.saturating_add(estimated_cell_change_bytes(change));
                if estimated_bytes > MAX_CELL_PATCH_BYTES_PER_RESPONSE {
                    break;
                }
            }
        }
    }
    if cell_change_count <= MAX_CELL_CHANGES_PER_RESPONSE
        && estimated_bytes <= MAX_CELL_PATCH_BYTES_PER_RESPONSE
    {
        return patches;
    }

    let mut invalidated = BTreeSet::new();
    let mut bounded = Vec::new();
    for patch in patches {
        match patch {
            MutationPatch::Cells { changes } => {
                invalidated.extend(changes.into_iter().map(|change| change.sheet_index));
            }
            other => bounded.push(other),
        }
    }
    invalidated.retain(|sheet_index| *sheet_index < file_data.sheets.len());
    bounded.extend(
        invalidated
            .into_iter()
            .map(|sheet_index| MutationPatch::SheetInvalidated { sheet_index }),
    );
    bounded
}

fn project_patch_display_formats(
    file_data: &DocumentData,
    patches: Vec<MutationPatch>,
) -> Vec<MutationPatch> {
    patches
        .into_iter()
        .map(|patch| match patch {
            MutationPatch::Cells { changes } => MutationPatch::Cells {
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

fn push_cell_change_if_missing(
    cell_changes: &mut Vec<ProjectedCellChange>,
    change: ProjectedCellChange,
) {
    if !cell_changes.iter().any(|existing| {
        existing.sheet_index == change.sheet_index
            && existing.row == change.row
            && existing.col == change.col
    }) {
        cell_changes.push(change);
    }
}

fn projected_cell_changes(changes: Vec<DocumentCellChange>) -> Vec<ProjectedCellChange> {
    changes
        .into_iter()
        .map(|change| {
            ProjectedCellChange::new(change.sheet_index, change.row, change.col, change.value)
        })
        .collect()
}

fn estimated_cell_change_bytes(change: &ProjectedCellChange) -> usize {
    96usize
        .saturating_add(change.display.as_ref().map_or(0, String::len))
        .saturating_add(change.format.as_ref().map_or(0, |format| {
            format.number_format.as_ref().map_or(0, String::len)
                + format.style_id.as_ref().map_or(0, String::len)
        }))
        .saturating_add(estimated_cell_value_bytes(&change.value))
}

fn estimated_cell_value_bytes(value: &CellValue) -> usize {
    match value {
        CellValue::Null | CellValue::Number(_) | CellValue::Boolean(_) => 32,
        CellValue::String(value) => value.len().saturating_mul(6).saturating_add(32),
        CellValue::Formula {
            formula,
            cached_value,
            error,
        } => formula
            .len()
            .saturating_mul(6)
            .saturating_add(
                error
                    .as_ref()
                    .map_or(0, |value| value.len().saturating_mul(6)),
            )
            .saturating_add(estimated_cell_value_bytes(cached_value))
            .saturating_add(64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_data::DocumentSheet;
    use crate::editor_protocol::MAX_MUTATION_RESPONSE_BYTES;
    use std::collections::HashMap;

    #[test]
    fn oversized_cell_patch_becomes_sheet_invalidation() {
        let state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "book.xlsx".to_string(),
                sheets: vec![DocumentSheet::default()],
            },
            None,
        );
        let changes = (0..=MAX_CELL_CHANGES_PER_RESPONSE)
            .map(|row| ProjectedCellChange::new(0, row, 0, CellValue::Null))
            .collect();

        let response = mutation_outcome(&state, vec![MutationPatch::Cells { changes }]);

        assert!(matches!(
            response.patches.as_slice(),
            [MutationPatch::SheetInvalidated { sheet_index }] if *sheet_index == 0
        ));
    }

    #[test]
    fn oversized_cell_patch_bytes_become_sheet_invalidation() {
        let state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "book.xlsx".to_string(),
                sheets: vec![DocumentSheet::default()],
            },
            None,
        );
        let changes = vec![ProjectedCellChange::new(
            0,
            0,
            0,
            CellValue::String("x".repeat(MAX_CELL_PATCH_BYTES_PER_RESPONSE + 1)),
        )];

        let response = mutation_outcome(&state, vec![MutationPatch::Cells { changes }]);

        assert!(matches!(
            response.patches.as_slice(),
            [MutationPatch::SheetInvalidated { sheet_index }] if *sheet_index == 0
        ));
    }

    #[test]
    fn structural_response_does_not_clone_complete_layout_maps() {
        let state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "book.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    row_heights: Some(
                        (0..100_000)
                            .map(|index| (index, 24))
                            .collect::<HashMap<_, _>>(),
                    ),
                    ..DocumentSheet::default()
                }],
            },
            None,
        );

        let response = mutation_outcome(
            &state,
            vec![MutationPatch::RowInserted {
                sheet_index: 0,
                row_index: 10,
                count: 1,
            }],
        );

        assert!(matches!(
            response.patches.as_slice(),
            [MutationPatch::RowInserted { .. }]
        ));
        let wire = crate::protocol_projection::mutation_response(response);
        assert!(serde_json::to_vec(&wire).unwrap().len() < 64 * 1024);
    }

    #[test]
    fn oversized_mutation_response_becomes_resync_required() {
        let state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "book.xlsx".to_string(),
                sheets: vec![DocumentSheet::default()],
            },
            None,
        );
        let response = mutation_outcome(
            &state,
            vec![MutationPatch::SheetsReplaced {
                start_index: 0,
                sheets: vec![SheetManifestSnapshot {
                    name: "x".repeat(MAX_MUTATION_RESPONSE_BYTES + 1),
                    extent: SheetExtent::default(),
                    layout: SheetLayoutSnapshot::default(),
                }],
            }],
        );
        let response = crate::protocol_projection::mutation_response(response);

        assert!(matches!(
            response.patches.as_slice(),
            [crate::types::EditorPatch::ResyncRequired { .. }]
        ));
        assert!(serde_json::to_vec(&response).unwrap().len() <= MAX_MUTATION_RESPONSE_BYTES);
    }
}
