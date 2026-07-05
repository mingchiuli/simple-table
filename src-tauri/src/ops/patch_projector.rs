use crate::state::editor_state::{EditorState, ExecutedOperation};
use crate::state::state::EditorStateInfo;
use crate::types::{
    AppliedOperationResult, CellValue, ColumnsDeletedPatch, ColumnsInsertedPatch,
    EditorMutationResponse, EditorPatch, FileData, LayoutPatch, ResyncRequiredPatch,
    RowsDeletedPatch, RowsInsertedPatch, SheetCellChange, SheetData, SheetDeletedPatch,
    SheetInsertedPatch, SheetMetadataPatch,
};

const EDITOR_MUTATION_PROTOCOL_VERSION: u16 = 1;

pub fn editor_state_info(editor_state: &EditorState) -> EditorStateInfo {
    EditorStateInfo {
        can_undo: editor_state.can_undo(),
        can_redo: editor_state.can_redo(),
        is_dirty: editor_state.is_dirty(),
    }
}

pub fn mutation_response(
    editor_state: &EditorState,
    patches: Vec<EditorPatch>,
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
) -> EditorMutationResponse {
    let mut patches = structural_patches(editor_state.file_data(), operation);

    if !cell_changes.is_empty() {
        patches.push(EditorPatch::Cells {
            changes: cell_changes,
        });
    }

    mutation_response(editor_state, patches)
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

    mutation_response(editor_state, restore.patches)
}

pub fn structural_patches(
    file_data: &FileData,
    operation: &AppliedOperationResult,
) -> Vec<EditorPatch> {
    match operation {
        AppliedOperationResult::AddRow {
            sheet_index, row, ..
        } => inserted_row_patches(file_data, *sheet_index, row.index),
        AppliedOperationResult::DeleteRow {
            sheet_index,
            row_index,
        } => deleted_row_patches(file_data, *sheet_index, *row_index, 1),
        AppliedOperationResult::AddColumn {
            sheet_index,
            column,
            ..
        } => inserted_column_patches(file_data, *sheet_index, column.index),
        AppliedOperationResult::DeleteColumn {
            sheet_index,
            column_index,
        } => deleted_column_patches(file_data, *sheet_index, *column_index, 1),
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

pub(crate) fn inserted_row_patches(
    file_data: &FileData,
    sheet_index: usize,
    row_index: usize,
) -> Vec<EditorPatch> {
    file_data
        .sheets
        .get(sheet_index)
        .map(|sheet| {
            vec![
                rows_inserted_patch(sheet_index, row_index, sheet),
                sheet_metadata_patch(sheet_index, sheet),
            ]
        })
        .unwrap_or_default()
}

pub(crate) fn deleted_row_patches(
    file_data: &FileData,
    sheet_index: usize,
    row_index: usize,
    count: usize,
) -> Vec<EditorPatch> {
    file_data
        .sheets
        .get(sheet_index)
        .map(|sheet| {
            vec![
                rows_deleted_patch(sheet_index, row_index, count),
                sheet_metadata_patch(sheet_index, sheet),
            ]
        })
        .unwrap_or_default()
}

pub(crate) fn inserted_column_patches(
    file_data: &FileData,
    sheet_index: usize,
    col_index: usize,
) -> Vec<EditorPatch> {
    file_data
        .sheets
        .get(sheet_index)
        .map(|sheet| {
            vec![
                columns_inserted_patch(sheet_index, col_index, sheet),
                sheet_metadata_patch(sheet_index, sheet),
            ]
        })
        .unwrap_or_default()
}

pub(crate) fn deleted_column_patches(
    file_data: &FileData,
    sheet_index: usize,
    col_index: usize,
    count: usize,
) -> Vec<EditorPatch> {
    file_data
        .sheets
        .get(sheet_index)
        .map(|sheet| {
            vec![
                columns_deleted_patch(sheet_index, col_index, count),
                sheet_metadata_patch(sheet_index, sheet),
            ]
        })
        .unwrap_or_default()
}

fn rows_inserted_patch(sheet_index: usize, row_index: usize, sheet: &SheetData) -> EditorPatch {
    EditorPatch::RowsInserted {
        patch: RowsInsertedPatch {
            sheet_index,
            row_index,
            rows: sheet
                .rows
                .get(row_index)
                .cloned()
                .map(|row| vec![row])
                .unwrap_or_default(),
            display_formats: Vec::new(),
            displays: Vec::new(),
            formats: Vec::new(),
            styles: Vec::new(),
        },
    }
}

fn rows_deleted_patch(sheet_index: usize, row_index: usize, count: usize) -> EditorPatch {
    EditorPatch::RowsDeleted {
        patch: RowsDeletedPatch {
            sheet_index,
            row_index,
            count,
        },
    }
}

fn columns_inserted_patch(sheet_index: usize, col_index: usize, sheet: &SheetData) -> EditorPatch {
    EditorPatch::ColumnsInserted {
        patch: ColumnsInsertedPatch {
            sheet_index,
            col_index,
            values: sheet
                .rows
                .iter()
                .map(|row| row.get(col_index).cloned().unwrap_or(CellValue::Null))
                .collect(),
            display_formats: Vec::new(),
            displays: Vec::new(),
            formats: Vec::new(),
            styles: Vec::new(),
        },
    }
}

fn columns_deleted_patch(sheet_index: usize, col_index: usize, count: usize) -> EditorPatch {
    EditorPatch::ColumnsDeleted {
        patch: ColumnsDeletedPatch {
            sheet_index,
            col_index,
            count,
        },
    }
}

pub fn sheet_metadata_patch(sheet_index: usize, sheet: &SheetData) -> EditorPatch {
    EditorPatch::SheetMetadata {
        patch: SheetMetadataPatch {
            sheet_index,
            merges: sheet.merges.clone(),
            column_widths: sheet.column_widths.clone().unwrap_or_default(),
            row_heights: sheet.row_heights.clone().unwrap_or_default(),
            rich: sheet.rich.clone(),
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
            EditorPatch::RowsInserted { mut patch } => {
                let sheet = file_data.sheets.get(patch.sheet_index);
                patch.display_formats =
                    patch_rows_metadata(&patch.rows, sheet, patch.row_index, |sheet, row, col| {
                        sheet.cell_format_at(row, col)
                    });
                patch.formats = patch.display_formats.clone();
                patch.styles =
                    patch_rows_metadata(&patch.rows, sheet, patch.row_index, |sheet, row, col| {
                        sheet.cell_style_at(row, col)
                    });
                patch.displays = patch
                    .rows
                    .iter()
                    .enumerate()
                    .map(|(row_offset, row)| {
                        row.iter()
                            .enumerate()
                            .map(|(col, cell)| {
                                sheet
                                    .map(|sheet| {
                                        sheet.cell_display_text(patch.row_index + row_offset, col)
                                    })
                                    .unwrap_or_else(|| cell.to_display_string())
                            })
                            .collect()
                    })
                    .collect();
                EditorPatch::RowsInserted { patch }
            }
            EditorPatch::ColumnsInserted { mut patch } => {
                let sheet = file_data.sheets.get(patch.sheet_index);
                patch.display_formats = patch
                    .values
                    .iter()
                    .enumerate()
                    .map(|(row, _)| {
                        sheet.and_then(|sheet| sheet.cell_format_at(row, patch.col_index))
                    })
                    .collect();
                patch.formats = patch.display_formats.clone();
                patch.styles = patch
                    .values
                    .iter()
                    .enumerate()
                    .map(|(row, _)| {
                        sheet.and_then(|sheet| sheet.cell_style_at(row, patch.col_index))
                    })
                    .collect();
                patch.displays = patch
                    .values
                    .iter()
                    .enumerate()
                    .map(|(row, cell)| {
                        sheet
                            .map(|sheet| sheet.cell_display_text(row, patch.col_index))
                            .unwrap_or_else(|| cell.to_display_string())
                    })
                    .collect();
                EditorPatch::ColumnsInserted { patch }
            }
            other => other,
        })
        .collect()
}

fn patch_rows_metadata<T>(
    rows: &[Vec<CellValue>],
    sheet: Option<&SheetData>,
    row_index: usize,
    read: impl Fn(&SheetData, usize, usize) -> Option<T>,
) -> Vec<Vec<Option<T>>> {
    rows.iter()
        .enumerate()
        .map(|(row_offset, row)| {
            row.iter()
                .enumerate()
                .map(|(col, _)| sheet.and_then(|sheet| read(sheet, row_index + row_offset, col)))
                .collect()
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
