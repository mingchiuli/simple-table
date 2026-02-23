use std::sync::Arc;
use std::sync::RwLock;

use crate::command::EditorState;
use crate::error::AppError;
use crate::types::OperationResult;
use crate::state::EditorStateInfo;

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
    let mut state = state.write().unwrap();
    match state.as_mut() {
        Some(editor_state) => {
            if let Some(result) = editor_state.undo() {
                Ok(result)
            } else {
                Err(AppError::Internal("Nothing to undo".to_string()))
            }
        }
        None => Err(AppError::Internal("No file loaded".to_string())),
    }
}

/// 重做操作
pub fn do_redo(state: Arc<RwLock<Option<EditorState>>>) -> Result<OperationResult, AppError> {
    let mut state = state.write().unwrap();
    match state.as_mut() {
        Some(editor_state) => {
            if let Some(result) = editor_state.redo() {
                Ok(result)
            } else {
                Err(AppError::Internal("Nothing to redo".to_string()))
            }
        }
        None => Err(AppError::Internal("No file loaded".to_string())),
    }
}
