use std::sync::{Arc, RwLock};

use crate::error::AppError;
use crate::ops::index_ops::spawn_rebuild_all_sheets_index;
use crate::state::editor_state::EditorState;
use crate::state::state::EditorStateInfo;
use crate::types::{
    EditorMutationResponse, EditorMutationResponseKind, OperationResult, SheetCellChange,
};

/// 获取编辑器状态信息
pub fn editor_state_info(editor_state: &EditorState) -> EditorStateInfo {
    EditorStateInfo {
        can_undo: editor_state.can_undo,
        can_redo: editor_state.can_redo,
        is_dirty: editor_state.is_dirty(),
    }
}

pub fn snapshot_mutation_response(
    editor_state: &EditorState,
    operation: Option<OperationResult>,
) -> EditorMutationResponse {
    EditorMutationResponse {
        kind: EditorMutationResponseKind::Snapshot,
        file_data: Some(editor_state.file_data.clone()),
        editor_state: editor_state_info(editor_state),
        operation,
        cell_changes: Vec::new(),
    }
}

pub fn cell_delta_mutation_response(
    editor_state: &EditorState,
    operation: OperationResult,
    mut cell_changes: Vec<SheetCellChange>,
) -> EditorMutationResponse {
    if let OperationResult::SetCell { sheet_index, cell } = &operation {
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

    EditorMutationResponse {
        kind: EditorMutationResponseKind::CellDelta,
        file_data: None,
        editor_state: editor_state_info(editor_state),
        operation: Some(operation),
        cell_changes,
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

fn get_editor_state_info(state: &Arc<RwLock<Option<EditorState>>>) -> Option<EditorStateInfo> {
    let state = state.read().expect("Editor state lock poisoned");
    state.as_ref().map(editor_state_info)
}

/// 获取编辑器状态（包含能否撤销/重做）
pub fn do_get_editor_state(
    state: Arc<RwLock<Option<EditorState>>>,
) -> Result<Option<EditorStateInfo>, AppError> {
    Ok(get_editor_state_info(&state))
}

/// 标记当前编辑器内容已经成功保存
pub fn do_mark_file_saved(state: Arc<RwLock<Option<EditorState>>>) -> Result<(), AppError> {
    let mut state = state.write().expect("Editor state lock poisoned");
    match state.as_mut() {
        Some(editor_state) => {
            editor_state.mark_saved();
            Ok(())
        }
        None => Err(AppError::NoFileLoaded),
    }
}

/// 撤销操作
pub fn do_undo(
    state: Arc<RwLock<Option<EditorState>>>,
) -> Result<EditorMutationResponse, AppError> {
    let response = {
        let mut state = state.write().expect("Editor state lock poisoned");
        match state.as_mut() {
            Some(editor_state) => {
                if let Some(result) = editor_state.undo() {
                    snapshot_mutation_response(editor_state, Some(result.operation))
                } else {
                    return Err(AppError::NothingToUndo);
                }
            }
            None => return Err(AppError::NoFileLoaded),
        }
    };

    // 异步重建索引
    spawn_rebuild_all_sheets_index(state);

    Ok(response)
}

/// 重做操作
pub fn do_redo(
    state: Arc<RwLock<Option<EditorState>>>,
) -> Result<EditorMutationResponse, AppError> {
    let response = {
        let mut state = state.write().expect("Editor state lock poisoned");
        match state.as_mut() {
            Some(editor_state) => {
                if let Some(result) = editor_state.redo() {
                    snapshot_mutation_response(editor_state, Some(result.operation))
                } else {
                    return Err(AppError::NothingToRedo);
                }
            }
            None => return Err(AppError::NoFileLoaded),
        }
    };

    // 异步重建索引
    spawn_rebuild_all_sheets_index(state);

    Ok(response)
}
