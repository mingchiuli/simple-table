use std::sync::Arc;
use std::sync::RwLock;

use crate::editor_state::EditorState;
use crate::index_ops::spawn_rebuild_sheet_index;
use crate::error::AppError;
use crate::types::OperationResult;
use crate::state::EditorStateInfo;

/// 从 OperationResult 中提取 sheet_index
fn extract_sheet_index(result: &OperationResult) -> usize {
    match result {
        OperationResult::SetCell { sheet_index, .. } => *sheet_index,
        OperationResult::AddRow { sheet_index, .. } => *sheet_index,
        OperationResult::DeleteRow { sheet_index, .. } => *sheet_index,
        OperationResult::AddColumn { sheet_index, .. } => *sheet_index,
        OperationResult::DeleteColumn { sheet_index, .. } => *sheet_index,
        OperationResult::AddSheet { sheet_index, .. } => *sheet_index,
        OperationResult::DeleteSheet { sheet_index } => *sheet_index,
        OperationResult::Batch { sheet_index, .. } => *sheet_index,
    }
}

/// 获取编辑器状态信息
fn get_editor_state_info(state: &Arc<RwLock<Option<EditorState>>>) -> Option<EditorStateInfo> {
    let state = state.read().unwrap();
    state.as_ref().map(|s| EditorStateInfo {
        can_undo: s.can_undo,
        can_redo: s.can_redo,
    })
}

/// 获取编辑器状态（包含能否撤销/重做）
pub fn do_get_editor_state(state: Arc<RwLock<Option<EditorState>>>) -> Result<Option<EditorStateInfo>, AppError> {
    Ok(get_editor_state_info(&state))
}

/// 撤销操作
pub fn do_undo(state: Arc<RwLock<Option<EditorState>>>) -> Result<OperationResult, AppError> {
    let sheet_index = {
        let mut state = state.write().unwrap();
        match state.as_mut() {
            Some(editor_state) => {
                if let Some(result) = editor_state.undo() {
                    let idx = extract_sheet_index(&result);
                    (result, idx)
                } else {
                    return Err(AppError::Internal("Nothing to undo".to_string()));
                }
            }
            None => return Err(AppError::Internal("No file loaded".to_string())),
        }
    };

    // 异步重建索引
    spawn_rebuild_sheet_index(sheet_index.1, state);

    Ok(sheet_index.0)
}

/// 重做操作
pub fn do_redo(state: Arc<RwLock<Option<EditorState>>>) -> Result<OperationResult, AppError> {
    let sheet_index = {
        let mut state = state.write().unwrap();
        match state.as_mut() {
            Some(editor_state) => {
                if let Some(result) = editor_state.redo() {
                    let idx = extract_sheet_index(&result);
                    (result, idx)
                } else {
                    return Err(AppError::Internal("Nothing to redo".to_string()));
                }
            }
            None => return Err(AppError::Internal("No file loaded".to_string())),
        }
    };

    // 异步重建索引
    spawn_rebuild_sheet_index(sheet_index.1, state);

    Ok(sheet_index.0)
}
