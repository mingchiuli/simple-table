use std::sync::{Arc, RwLock};

use crate::error::AppError;
use crate::ops::index_ops::schedule_index_for_response;
use crate::ops::patch_projector::{editor_state_info, restore_mutation_response};
use crate::state::state::{ActiveDocumentStore, EditorSessionInfo};
use crate::types::EditorMutationResponse;

/// 获取编辑器状态（包含能否撤销/重做）
pub fn do_get_editor_state(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: Option<u64>,
    base_revision: Option<u64>,
) -> Result<Option<EditorSessionInfo>, AppError> {
    let registry = registry
        .read()
        .map_err(|_| AppError::poisoned_lock("document registry"))?;

    match (document_id, base_revision) {
        (Some(document_id), Some(base_revision)) => registry
            .active_for_command(document_id, base_revision)
            .map(|editor_state| {
                Some(EditorSessionInfo {
                    document_id: editor_state.document_id(),
                    revision: editor_state.revision(),
                    formula_status: editor_state.formula_status(),
                    capabilities: editor_state.capabilities(),
                    editor_state: editor_state_info(editor_state),
                })
            }),
        (None, None) => Ok(None),
        _ => Err(AppError::DocumentStateInvalid(
            "document state request must include both documentId and baseRevision".to_string(),
        )),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::editor_state::EditorState;
    use crate::types::{CellValue, FileData, SheetData};

    fn make_registry() -> Arc<RwLock<ActiveDocumentStore>> {
        let editor = EditorState::with_workbook(
            FileData {
                path: String::new(),
                file_name: "test.xlsx".to_string(),
                sheets: vec![SheetData {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::String("A1".to_string())]],
                    ..Default::default()
                }],
            },
            None,
        );
        let mut registry = ActiveDocumentStore::new_for_test();
        registry.replace_active(editor);
        Arc::new(RwLock::new(registry))
    }

    fn command_session(registry: &Arc<RwLock<ActiveDocumentStore>>) -> (u64, u64) {
        let guard = registry.read().expect("registry");
        let editor = guard.active().expect("active document");
        (editor.document_id(), editor.revision())
    }

    #[test]
    fn get_editor_state_rejects_stale_context() {
        let registry = make_registry();
        let (document_id, revision) = command_session(&registry);
        crate::ops::cell_ops::do_set_cell(
            &registry,
            document_id,
            revision,
            0,
            0,
            0,
            "changed".to_string(),
        )
        .expect("edit");

        let error = do_get_editor_state(&registry, Some(document_id), Some(revision))
            .expect_err("stale state request");

        assert!(matches!(error, AppError::DocumentStateInvalid(_)));
    }

    #[test]
    fn get_editor_state_requires_complete_context() {
        let registry = make_registry();
        let (document_id, _) = command_session(&registry);

        let error = do_get_editor_state(&registry, Some(document_id), None)
            .expect_err("partial state request");

        assert!(matches!(error, AppError::DocumentStateInvalid(_)));
    }

    #[test]
    fn get_editor_state_without_context_returns_no_session() {
        let registry = make_registry();

        let session = do_get_editor_state(&registry, None, None).expect("state request");

        assert!(session.is_none());
    }
}
