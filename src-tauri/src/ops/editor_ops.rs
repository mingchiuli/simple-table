use std::sync::{Arc, RwLock};

use crate::error::AppError;
use crate::ops::index_ops::schedule_index_for_response;
use crate::state::editor_state::EditorState;
use crate::state::state::{ActiveDocumentStore, EditorSessionInfo, EditorStateInfo};
use crate::types::{
    AppliedOperationResult, EditorMutationResponse, EditorPatch, LayoutPatch, SheetCellChange,
    SheetDeletedPatch, SheetInsertedPatch,
};

const EDITOR_MUTATION_PROTOCOL_VERSION: u16 = 1;

/// 获取编辑器状态信息
pub fn editor_state_info(editor_state: &EditorState) -> EditorStateInfo {
    EditorStateInfo {
        can_undo: editor_state.can_undo,
        can_redo: editor_state.can_redo,
        is_dirty: editor_state.is_dirty(),
    }
}

fn mutation_response(
    editor_state: &EditorState,
    patches: Vec<EditorPatch>,
) -> EditorMutationResponse {
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

pub fn snapshot_mutation_response(
    editor_state: &EditorState,
    _operation: Option<AppliedOperationResult>,
) -> EditorMutationResponse {
    mutation_response(
        editor_state,
        vec![EditorPatch::FullSnapshot {
            file_data: editor_state.file_data().clone(),
        }],
    )
}

pub fn cell_delta_mutation_response(
    editor_state: &EditorState,
    operation: AppliedOperationResult,
    mut cell_changes: Vec<SheetCellChange>,
) -> EditorMutationResponse {
    if let AppliedOperationResult::SetCell { sheet_index, cell } = &operation {
        push_cell_change_if_missing(
            &mut cell_changes,
            SheetCellChange {
                sheet_index: *sheet_index,
                row: cell.row,
                col: cell.col,
                value: cell.value.clone(),
            },
        );
    }
    if let AppliedOperationResult::SetCells { changes } = &operation {
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
    operation: AppliedOperationResult,
    cell_changes: Vec<SheetCellChange>,
) -> EditorMutationResponse {
    let mut patches = structural_patches(editor_state, operation);

    if !cell_changes.is_empty() {
        patches.push(EditorPatch::Cells {
            changes: cell_changes,
        });
    }

    mutation_response(editor_state, patches)
}

fn structural_patches(
    editor_state: &EditorState,
    operation: AppliedOperationResult,
) -> Vec<EditorPatch> {
    match operation {
        AppliedOperationResult::AddRow { sheet_index, .. }
        | AppliedOperationResult::DeleteRow { sheet_index, .. }
        | AppliedOperationResult::AddColumn { sheet_index, .. }
        | AppliedOperationResult::DeleteColumn { sheet_index, .. } => editor_state
            .file_data()
            .sheets
            .get(sheet_index)
            .map(|sheet| {
                vec![EditorPatch::SheetSnapshot {
                    sheet_index,
                    sheet: sheet.clone(),
                }]
            })
            .unwrap_or_default(),
        AppliedOperationResult::AddSheet {
            sheet_index,
            sheet_data,
            ..
        } => vec![EditorPatch::SheetInserted {
            patch: SheetInsertedPatch {
                sheet_index,
                sheet: sheet_data,
            },
        }],
        AppliedOperationResult::DeleteSheet { sheet_index, .. } => {
            vec![EditorPatch::SheetDeleted {
                patch: SheetDeletedPatch { sheet_index },
            }]
        }
        AppliedOperationResult::SetCell { .. }
        | AppliedOperationResult::SetCells { .. }
        | AppliedOperationResult::SetColumnWidth { .. }
        | AppliedOperationResult::SetRowHeight { .. } => Vec::new(),
    }
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

fn get_editor_session_info(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
) -> Option<EditorSessionInfo> {
    let registry = registry.read().expect("Document registry lock poisoned");
    registry.active().map(|editor_state| EditorSessionInfo {
        document_id: editor_state.document_id(),
        revision: editor_state.revision(),
        formula_status: editor_state.formula_status(),
        capabilities: editor_state.capabilities(),
        editor_state: editor_state_info(editor_state),
    })
}

/// 获取编辑器状态（包含能否撤销/重做）
pub fn do_get_editor_state(
    registry: Arc<RwLock<ActiveDocumentStore>>,
) -> Result<Option<EditorSessionInfo>, AppError> {
    Ok(get_editor_session_info(&registry))
}

/// 标记当前编辑器内容已经成功保存
pub fn do_mark_file_saved(registry: Arc<RwLock<ActiveDocumentStore>>) -> Result<(), AppError> {
    let mut registry = registry.write().expect("Document registry lock poisoned");
    match registry.active_mut() {
        Some(editor_state) => {
            editor_state.mark_saved();
            Ok(())
        }
        None => Err(AppError::NoFileLoaded),
    }
}

/// 撤销操作
pub fn do_undo(
    registry: Arc<RwLock<ActiveDocumentStore>>,
) -> Result<EditorMutationResponse, AppError> {
    let response = {
        let mut registry_guard = registry.write().expect("Document registry lock poisoned");
        match registry_guard.active_mut() {
            Some(editor_state) => {
                if let Some(result) = editor_state.undo()? {
                    snapshot_mutation_response(editor_state, result.operation)
                } else {
                    return Err(AppError::NothingToUndo);
                }
            }
            None => return Err(AppError::NoFileLoaded),
        }
    };

    schedule_index_for_response(&response, registry);

    Ok(response)
}

/// 重做操作
pub fn do_redo(
    registry: Arc<RwLock<ActiveDocumentStore>>,
) -> Result<EditorMutationResponse, AppError> {
    let response = {
        let mut registry_guard = registry.write().expect("Document registry lock poisoned");
        match registry_guard.active_mut() {
            Some(editor_state) => {
                if let Some(result) = editor_state.redo()? {
                    snapshot_mutation_response(editor_state, result.operation)
                } else {
                    return Err(AppError::NothingToRedo);
                }
            }
            None => return Err(AppError::NoFileLoaded),
        }
    };

    schedule_index_for_response(&response, registry);

    Ok(response)
}
