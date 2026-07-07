use std::sync::{Arc, RwLock};

use crate::error::AppError;
use crate::ops::index_ops::schedule_index_for_response;
use crate::ops::patch_projector::{editor_state_info, restore_mutation_response};
use crate::state::state::{ActiveDocumentStore, EditorSessionInfo};
use crate::types::EditorMutationResponse;

fn get_editor_session_info(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
) -> Result<Option<EditorSessionInfo>, AppError> {
    let registry = registry
        .read()
        .map_err(|_| AppError::poisoned_lock("document registry"))?;
    Ok(registry.active().map(|editor_state| EditorSessionInfo {
        document_id: editor_state.document_id(),
        revision: editor_state.revision(),
        formula_status: editor_state.formula_status(),
        capabilities: editor_state.capabilities(),
        editor_state: editor_state_info(editor_state),
    }))
}

/// 获取编辑器状态（包含能否撤销/重做）
pub fn do_get_editor_state(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
) -> Result<Option<EditorSessionInfo>, AppError> {
    get_editor_session_info(registry)
}

/// 撤销操作
pub fn do_undo(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
    base_revision: u64,
) -> Result<EditorMutationResponse, AppError> {
    let response = {
        let mut registry_guard = registry
            .write()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        let editor_state = registry_guard.active_mut_for_command(document_id, base_revision)?;
        if let Some(result) = editor_state.undo()? {
            restore_mutation_response(editor_state, result)
        } else {
            return Err(AppError::NothingToUndo);
        }
    };

    schedule_index_for_response(&response, registry);

    Ok(response)
}

/// 重做操作
pub fn do_redo(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
    base_revision: u64,
) -> Result<EditorMutationResponse, AppError> {
    let response = {
        let mut registry_guard = registry
            .write()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        let editor_state = registry_guard.active_mut_for_command(document_id, base_revision)?;
        if let Some(result) = editor_state.redo()? {
            restore_mutation_response(editor_state, result)
        } else {
            return Err(AppError::NothingToRedo);
        }
    };

    schedule_index_for_response(&response, registry);

    Ok(response)
}
