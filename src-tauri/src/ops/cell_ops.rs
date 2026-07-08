use std::sync::{Arc, RwLock};

use crate::error::AppError;
use crate::ops::EditorCommand;
use crate::ops::index_ops::schedule_index_for_response;
use crate::ops::patch_projector::{
    cell_delta_mutation_response, layout_mutation_response, resync_required_mutation_response,
    status_mutation_response, structural_delta_mutation_response,
};
use crate::state::state::ActiveDocumentStore;
use crate::types::{EditorMutationResponse, LayoutPatch, SetCellRequest};

pub fn do_set_cell(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    row: usize,
    col: usize,
    text: String,
) -> Result<EditorMutationResponse, AppError> {
    let response = execute_cell_delta(
        registry,
        document_id,
        base_revision,
        EditorCommand::SetCell {
            sheet_index,
            row,
            col,
            text,
        },
    );

    if let Ok(response) = &response {
        schedule_index_for_response(response, registry);
    }

    response
}

pub fn do_set_cells(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
    base_revision: u64,
    changes: Vec<SetCellRequest>,
) -> Result<EditorMutationResponse, AppError> {
    let response = execute_cell_delta(
        registry,
        document_id,
        base_revision,
        EditorCommand::SetCells { changes },
    );

    if let Ok(response) = &response {
        schedule_index_for_response(response, registry);
    }

    response
}

pub fn do_add_row(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    row_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    execute_structural_command(
        registry,
        document_id,
        base_revision,
        EditorCommand::AddRow {
            sheet_index,
            row_index,
        },
    )
}

pub fn do_delete_row(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    row_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    execute_structural_command(
        registry,
        document_id,
        base_revision,
        EditorCommand::DeleteRow {
            sheet_index,
            row_index,
        },
    )
}

pub fn do_add_column(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    col_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    execute_structural_command(
        registry,
        document_id,
        base_revision,
        EditorCommand::AddColumn {
            sheet_index,
            col_index,
        },
    )
}

pub fn do_delete_column(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    col_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    execute_structural_command(
        registry,
        document_id,
        base_revision,
        EditorCommand::DeleteColumn {
            sheet_index,
            col_index,
        },
    )
}

pub fn do_set_column_width(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    col_index: usize,
    width: Option<u32>,
) -> Result<EditorMutationResponse, AppError> {
    execute_layout(
        registry,
        document_id,
        base_revision,
        EditorCommand::SetColumnWidth {
            sheet_index,
            col_index,
            width,
        },
        column_width_patch(sheet_index, col_index, width),
    )
}

pub fn do_set_row_height(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    row_index: usize,
    height: Option<u32>,
) -> Result<EditorMutationResponse, AppError> {
    execute_layout(
        registry,
        document_id,
        base_revision,
        EditorCommand::SetRowHeight {
            sheet_index,
            row_index,
            height,
        },
        row_height_patch(sheet_index, row_index, height),
    )
}

pub fn do_add_sheet(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
    base_revision: u64,
) -> Result<EditorMutationResponse, AppError> {
    execute_structural_command(
        registry,
        document_id,
        base_revision,
        EditorCommand::AddSheet { name: None },
    )
}

pub fn do_delete_sheet(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    execute_structural_command(
        registry,
        document_id,
        base_revision,
        EditorCommand::DeleteSheet { sheet_index },
    )
}

fn execute_cell_delta(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
    base_revision: u64,
    command: EditorCommand,
) -> Result<EditorMutationResponse, AppError> {
    let mut registry_guard = registry
        .write()
        .map_err(|_| AppError::poisoned_lock("document registry"))?;
    let editor_state = registry_guard.active_mut_for_command(document_id, base_revision)?;
    let result = editor_state.execute(command)?;
    if let Some(operation) = result.operation {
        Ok(cell_delta_mutation_response(
            editor_state,
            &operation,
            result.cell_changes,
        ))
    } else {
        Ok(status_mutation_response(editor_state))
    }
}

fn execute_structural_command(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
    base_revision: u64,
    command: EditorCommand,
) -> Result<EditorMutationResponse, AppError> {
    let response = {
        let mut registry_guard = registry
            .write()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        let editor_state = registry_guard.active_mut_for_command(document_id, base_revision)?;
        let result = editor_state.execute(command)?;
        match result.operation {
            Some(operation) => structural_delta_mutation_response(
                editor_state,
                &operation,
                result.cell_changes,
                result.search_index_update,
            ),
            None => resync_required_mutation_response(
                editor_state,
                "structure edit completed without an operation result",
            ),
        }
    };

    schedule_index_for_response(&response, registry);

    Ok(response)
}

fn execute_layout(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
    base_revision: u64,
    command: EditorCommand,
    patch: LayoutPatch,
) -> Result<EditorMutationResponse, AppError> {
    let mut registry_guard = registry
        .write()
        .map_err(|_| AppError::poisoned_lock("document registry"))?;
    let editor_state = registry_guard.active_mut_for_command(document_id, base_revision)?;
    let result = editor_state.execute(command)?;
    if result.operation.is_some() {
        Ok(layout_mutation_response(editor_state, patch))
    } else {
        Ok(status_mutation_response(editor_state))
    }
}

fn column_width_patch(sheet_index: usize, col_index: usize, width: Option<u32>) -> LayoutPatch {
    LayoutPatch {
        sheet_index,
        column_widths: [(col_index, width)].into_iter().collect(),
        row_heights: Default::default(),
    }
}

fn row_height_patch(sheet_index: usize, row_index: usize, height: Option<u32>) -> LayoutPatch {
    LayoutPatch {
        sheet_index,
        column_widths: Default::default(),
        row_heights: [(row_index, height)].into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::editor_state::EditorState;
    use crate::state::state::ActiveDocumentStore;
    use crate::types::{
        CellFormatProjection, CellValue, EditorPatch, FileData, ReadOnlyRichProjection, SheetData,
    };
    use serde_json::Value;
    use std::collections::HashMap;

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

    fn make_formatted_registry() -> Arc<RwLock<ActiveDocumentStore>> {
        let editor = EditorState::with_workbook(
            FileData {
                path: String::new(),
                file_name: "test.xlsx".to_string(),
                sheets: vec![SheetData {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::Number(Value::from(0.4))]],
                    rich: ReadOnlyRichProjection {
                        cell_formats: HashMap::from([(
                            "A1".to_string(),
                            CellFormatProjection {
                                number_format: Some("0%".to_string()),
                                style_id: None,
                            },
                        )]),
                        ..Default::default()
                    },
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
    fn stale_editor_command_context_is_rejected() {
        let registry = make_registry();
        let (document_id, revision) = command_session(&registry);
        do_set_cell(
            &registry,
            document_id,
            revision,
            0,
            0,
            0,
            "changed".to_string(),
        )
        .expect("first edit");

        let error = do_set_cell(
            &registry,
            document_id,
            revision,
            0,
            0,
            0,
            "stale".to_string(),
        )
        .expect_err("stale command context");

        assert!(matches!(error, AppError::DocumentStateInvalid(_)));
    }

    #[test]
    fn row_and_column_structure_edits_return_delta_patches() {
        let registry = make_registry();
        let (document_id, revision) = command_session(&registry);
        let add_row_response = do_add_row(&registry, document_id, revision, 0, 1).expect("add row");
        assert!(matches!(
            add_row_response.patches.first(),
            Some(EditorPatch::RowInserted { patch })
                if patch.sheet_index == 0 && patch.row_index == 1 && patch.rows.len() == 1
        ));
        assert_eq!(add_row_response.patches.len(), 1);
        assert!(add_row_response.search_index_update.rebuild_all);

        let registry = make_registry();
        let (document_id, revision) = command_session(&registry);
        let add_column_response =
            do_add_column(&registry, document_id, revision, 0, 1).expect("add column");
        assert!(matches!(
            add_column_response.patches.first(),
            Some(EditorPatch::ColumnInserted { patch })
                if patch.sheet_index == 0 && patch.col_index == 1 && patch.values.len() == 1
        ));
        assert_eq!(add_column_response.patches.len(), 1);
        assert!(add_column_response.search_index_update.rebuild_all);
    }

    #[test]
    fn no_op_cell_edit_returns_status_only_response() {
        let registry = make_registry();
        let (document_id, revision) = command_session(&registry);
        let response = do_set_cell(&registry, document_id, revision, 0, 0, 0, "A1".to_string())
            .expect("set same cell");

        assert_eq!(response.revision, 0);
        assert!(response.patches.is_empty());
    }

    #[test]
    fn no_op_layout_edit_returns_status_only_response() {
        let registry = make_registry();
        let (document_id, revision) = command_session(&registry);
        let response = do_set_column_width(&registry, document_id, revision, 0, 0, None)
            .expect("clear default width");

        assert_eq!(response.revision, 0);
        assert!(response.patches.is_empty());
    }

    #[test]
    fn resize_rejects_indexes_outside_sheet_extent() {
        let registry = make_registry();
        let (document_id, revision) = command_session(&registry);

        assert!(do_set_column_width(&registry, document_id, revision, 0, 5, Some(120)).is_err());
        assert!(do_set_row_height(&registry, document_id, revision, 0, 5, Some(72)).is_err());
    }

    #[test]
    fn undo_returns_delta_patches_without_resync() {
        let registry = make_registry();
        let (document_id, revision) = command_session(&registry);
        do_set_cell(
            &registry,
            document_id,
            revision,
            0,
            0,
            0,
            "changed".to_string(),
        )
        .expect("set cell");

        let (document_id, revision) = command_session(&registry);
        let response =
            crate::ops::editor_ops::do_undo(&registry, document_id, revision).expect("undo");

        assert!(
            !response
                .patches
                .iter()
                .any(|patch| matches!(patch, EditorPatch::ResyncRequired { .. }))
        );
        assert!(
            response
                .patches
                .iter()
                .any(|patch| matches!(patch, EditorPatch::Cells { .. }))
        );
    }

    #[test]
    fn cell_delta_serializes_formatted_display_projection() {
        let registry = make_formatted_registry();
        let (document_id, revision) = command_session(&registry);
        let response = do_set_cell(&registry, document_id, revision, 0, 0, 0, "0.5".to_string())
            .expect("set formatted cell");
        let json = serde_json::to_value(response).expect("serialize response");

        assert_eq!(
            json["patches"][0]["data"]["changes"][0]["value"]["display"],
            "50%"
        );
        assert_eq!(
            json["patches"][0]["data"]["changes"][0]["value"]["format"]["numberFormat"],
            "0%"
        );
    }
}
