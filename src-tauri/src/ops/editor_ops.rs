use std::sync::{Arc, RwLock};

use crate::error::AppError;
use crate::ops::index_ops::schedule_index_for_response;
use crate::ops::patch_projector::{editor_state_info, restore_mutation_response};
use crate::state::state::{ActiveDocumentStore, EditorSessionInfo};
use crate::types::EditorMutationResponse;

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
                    restore_mutation_response(editor_state, result)
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
                    restore_mutation_response(editor_state, result)
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
