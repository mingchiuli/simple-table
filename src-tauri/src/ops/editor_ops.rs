use crate::error::AppError;
use crate::ops::patch_projector::{editor_state_info, restore_mutation_response};
use crate::state::state::ActiveDocumentRepository;
use crate::types::{EditorMutationResponse, EditorSessionInfo};

/// 获取编辑器状态（包含能否撤销/重做）
pub fn do_get_editor_state(
    registry: &ActiveDocumentRepository,
    document_id: Option<u64>,
    base_revision: Option<u64>,
) -> Result<Option<EditorSessionInfo>, AppError> {
    match (document_id, base_revision) {
        (Some(document_id), Some(base_revision)) => {
            let handle = registry.read_handle(document_id)?;
            let editor_state = handle.read_for_command(document_id, base_revision)?;
            Ok(Some(EditorSessionInfo {
                document_id: editor_state.document_id(),
                revision: editor_state.revision(),
                formula_status: editor_state.formula_status(),
                capabilities: editor_state.capabilities(),
                editor_state: editor_state_info(&editor_state),
            }))
        }
        (None, None) => Ok(None),
        _ => Err(AppError::DocumentStateInvalid(
            "document state request must include both documentId and baseRevision".to_string(),
        )),
    }
}

/// 撤销操作
pub fn do_undo(
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
) -> Result<EditorMutationResponse, AppError> {
    let handle = registry.mutation_handle(document_id)?;
    let (response, retired) = {
        let mut editor_state = handle.write_for_command(document_id, base_revision)?;
        if let Some(result) = editor_state.undo()? {
            let response = restore_mutation_response(
                &editor_state,
                result.restore,
                result.search_index_update,
            );
            (response, result.retired)
        } else {
            return Err(AppError::NothingToUndo);
        }
    };
    drop(retired);

    Ok(response)
}

/// 重做操作
pub fn do_redo(
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
) -> Result<EditorMutationResponse, AppError> {
    let handle = registry.mutation_handle(document_id)?;
    let (response, retired) = {
        let mut editor_state = handle.write_for_command(document_id, base_revision)?;
        if let Some(result) = editor_state.redo()? {
            let response = restore_mutation_response(
                &editor_state,
                result.restore,
                result.search_index_update,
            );
            (response, result.retired)
        } else {
            return Err(AppError::NothingToRedo);
        }
    };
    drop(retired);

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::editor_state::EditorState;
    use crate::types::{CellValue, FileData, SheetData};

    fn make_registry() -> ActiveDocumentRepository {
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
        let registry = ActiveDocumentRepository::default();
        registry.replace_active_for_test(editor);
        registry
    }

    fn command_session(registry: &ActiveDocumentRepository) -> (u64, u64) {
        let handle = registry
            .active_handle()
            .expect("registry")
            .expect("active document");
        let editor = handle.read().expect("document state");
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
