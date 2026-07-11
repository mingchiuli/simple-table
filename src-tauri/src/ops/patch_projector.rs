use crate::state::editor_state::{EditorState, ExecutedOperation};
use crate::state::state::EditorStateInfo;
use crate::types::{
    AppliedOperationResult, ColumnDeletedPatch, ColumnInsertedPatch, EditorMutationResponse,
    EditorPatch, FileData, LayoutPatch, ResyncRequiredPatch, RowDeletedPatch, RowInsertedPatch,
    SearchIndexUpdatePlan, SheetCellChange, SheetDeletedPatch, SheetInsertedPatch,
    SheetInvalidatedPatch, SheetManifest,
};
use std::collections::BTreeSet;

const EDITOR_MUTATION_PROTOCOL_VERSION: u16 = 2;
const MAX_CELL_CHANGES_PER_RESPONSE: usize = 4_096;
const MAX_CELL_PATCH_BYTES_PER_RESPONSE: usize = 2 * 1024 * 1024;

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
    let patches = bounded_patches(
        editor_state.file_data(),
        project_patch_display_formats(editor_state.file_data(), patches),
    );
    EditorMutationResponse {
        protocol_version: EDITOR_MUTATION_PROTOCOL_VERSION,
        document_id: editor_state.document_id(),
        revision: editor_state.revision(),
        formula_status: editor_state.formula_status().bounded(100),
        capabilities: editor_state.capabilities(),
        editor_state: editor_state_info(editor_state),
        patches,
        sheet_extents: Some(editor_state.sheet_extents()),
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
    _file_data: &FileData,
    operation: &AppliedOperationResult,
) -> Vec<EditorPatch> {
    match operation {
        AppliedOperationResult::AddRow { sheet_index, row } => vec![EditorPatch::RowInserted {
            patch: RowInsertedPatch {
                sheet_index: *sheet_index,
                row_index: row.index,
                count: 1,
            },
        }],
        AppliedOperationResult::DeleteRow {
            sheet_index,
            row_index,
        } => vec![EditorPatch::RowDeleted {
            patch: RowDeletedPatch {
                sheet_index: *sheet_index,
                row_index: *row_index,
                count: 1,
            },
        }],
        AppliedOperationResult::AddColumn {
            sheet_index,
            column,
            ..
        } => vec![EditorPatch::ColumnInserted {
            patch: ColumnInsertedPatch {
                sheet_index: *sheet_index,
                col_index: column.index,
                count: 1,
            },
        }],
        AppliedOperationResult::DeleteColumn {
            sheet_index,
            column_index,
        } => vec![EditorPatch::ColumnDeleted {
            patch: ColumnDeletedPatch {
                sheet_index: *sheet_index,
                col_index: *column_index,
                count: 1,
            },
        }],
        AppliedOperationResult::AddSheet {
            sheet_index,
            sheet_data,
            ..
        } => vec![EditorPatch::SheetInserted {
            patch: SheetInsertedPatch {
                sheet_index: *sheet_index,
                sheet: SheetManifest {
                    name: sheet_data.name.clone(),
                    extent: sheet_data.extent(),
                },
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

pub(crate) fn sheet_invalidated_patch(sheet_index: usize) -> Vec<EditorPatch> {
    vec![EditorPatch::SheetInvalidated {
        patch: SheetInvalidatedPatch { sheet_index },
    }]
}

fn bounded_patches(file_data: &FileData, patches: Vec<EditorPatch>) -> Vec<EditorPatch> {
    let mut cell_change_count = 0usize;
    let mut estimated_bytes = 0usize;
    for patch in &patches {
        if let EditorPatch::Cells { changes } = patch {
            cell_change_count = cell_change_count.saturating_add(changes.len());
            for change in changes {
                estimated_bytes = estimated_bytes.saturating_add(
                    serde_json::to_vec(change)
                        .map_or(MAX_CELL_PATCH_BYTES_PER_RESPONSE + 1, |v| v.len()),
                );
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
            EditorPatch::Cells { changes } => {
                invalidated.extend(changes.into_iter().map(|change| change.sheet_index));
            }
            other => bounded.push(other),
        }
    }
    invalidated.retain(|sheet_index| *sheet_index < file_data.sheets.len());
    bounded.extend(
        invalidated
            .into_iter()
            .map(|sheet_index| EditorPatch::SheetInvalidated {
                patch: SheetInvalidatedPatch { sheet_index },
            }),
    );
    bounded
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CellValue, SheetData};

    #[test]
    fn oversized_cell_patch_becomes_sheet_invalidation() {
        let state = EditorState::with_workbook(
            FileData {
                path: String::new(),
                file_name: "book.xlsx".to_string(),
                sheets: vec![SheetData::default()],
            },
            None,
        );
        let changes = (0..=MAX_CELL_CHANGES_PER_RESPONSE)
            .map(|row| SheetCellChange::new(0, row, 0, CellValue::Null))
            .collect();

        let response = mutation_response(&state, vec![EditorPatch::Cells { changes }]);

        assert!(matches!(
            response.patches.as_slice(),
            [EditorPatch::SheetInvalidated { patch }] if patch.sheet_index == 0
        ));
    }

    #[test]
    fn oversized_cell_patch_bytes_become_sheet_invalidation() {
        let state = EditorState::with_workbook(
            FileData {
                path: String::new(),
                file_name: "book.xlsx".to_string(),
                sheets: vec![SheetData::default()],
            },
            None,
        );
        let changes = vec![SheetCellChange::new(
            0,
            0,
            0,
            CellValue::String("x".repeat(MAX_CELL_PATCH_BYTES_PER_RESPONSE + 1)),
        )];

        let response = mutation_response(&state, vec![EditorPatch::Cells { changes }]);

        assert!(matches!(
            response.patches.as_slice(),
            [EditorPatch::SheetInvalidated { patch }] if patch.sheet_index == 0
        ));
    }
}
