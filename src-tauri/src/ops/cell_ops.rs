use crate::domain::{CellEditInput, EditorCommand};
use crate::domain::{SearchCellIndexUpdate, SearchIndexWork};
use crate::error::AppError;
use crate::ops::mutation_execution::MutationExecution;
use crate::ops::patch_projector::{
    cell_delta_mutation_response, complete_cell_changes, layout_mutation_response,
    resync_required_mutation_response, status_mutation_response,
    structural_delta_mutation_response,
};
use crate::state::state::ActiveDocumentRepository;
use crate::types::display::DisplayProjection;
use crate::types::{LayoutPatch, SheetCellChange};

pub fn do_set_cell(
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    row: usize,
    col: usize,
    text: String,
) -> Result<MutationExecution, AppError> {
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

    response
}

pub fn do_set_cells(
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
    changes: Vec<CellEditInput>,
) -> Result<MutationExecution, AppError> {
    let response = execute_cell_delta(
        registry,
        document_id,
        base_revision,
        EditorCommand::SetCells { changes },
    );

    response
}

pub fn do_add_row(
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    row_index: usize,
) -> Result<MutationExecution, AppError> {
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
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    row_index: usize,
) -> Result<MutationExecution, AppError> {
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
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    col_index: usize,
) -> Result<MutationExecution, AppError> {
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
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    col_index: usize,
) -> Result<MutationExecution, AppError> {
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
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    col_index: usize,
    width: Option<u32>,
) -> Result<MutationExecution, AppError> {
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
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    row_index: usize,
    height: Option<u32>,
) -> Result<MutationExecution, AppError> {
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
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
) -> Result<MutationExecution, AppError> {
    execute_structural_command(
        registry,
        document_id,
        base_revision,
        EditorCommand::AddSheet { name: None },
    )
}

pub fn do_delete_sheet(
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
) -> Result<MutationExecution, AppError> {
    execute_structural_command(
        registry,
        document_id,
        base_revision,
        EditorCommand::DeleteSheet { sheet_index },
    )
}

fn execute_cell_delta(
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
    command: EditorCommand,
) -> Result<MutationExecution, AppError> {
    let handle = mutation_handle(registry, document_id)?;
    let (execution, retired) = {
        let mut editor_state = handle.write_for_command(document_id, base_revision)?;
        let result = editor_state.execute(command)?;
        let retired = result.retired;
        let execution = if let Some(operation) = result.operation {
            let changes = complete_cell_changes(&operation, result.cell_changes);
            let search_index_work = search_index_work_for_changes(&editor_state, &changes);
            MutationExecution::new(
                cell_delta_mutation_response(&editor_state, changes),
                search_index_work,
            )
        } else {
            MutationExecution::new(
                status_mutation_response(&editor_state),
                SearchIndexWork::None,
            )
        };
        (execution, retired)
    };
    drop(retired);
    Ok(execution)
}

fn execute_structural_command(
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
    command: EditorCommand,
) -> Result<MutationExecution, AppError> {
    let handle = mutation_handle(registry, document_id)?;
    let (execution, retired) = {
        let mut editor_state = handle.write_for_command(document_id, base_revision)?;
        let result = editor_state.execute(command)?;
        let retired = result.retired;
        let execution = match result.operation {
            Some(operation) => MutationExecution::new(
                structural_delta_mutation_response(&editor_state, &operation, result.cell_changes),
                result.search_index_work,
            ),
            None => MutationExecution::new(
                resync_required_mutation_response(
                    &editor_state,
                    "structure edit completed without an operation result",
                ),
                SearchIndexWork::RebuildAll,
            ),
        };
        (execution, retired)
    };
    drop(retired);

    Ok(execution)
}

fn execute_layout(
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
    command: EditorCommand,
    patch: LayoutPatch,
) -> Result<MutationExecution, AppError> {
    let handle = mutation_handle(registry, document_id)?;
    let (execution, retired) = {
        let mut editor_state = handle.write_for_command(document_id, base_revision)?;
        let result = editor_state.execute(command)?;
        let response = if result.operation.is_some() {
            layout_mutation_response(&editor_state, patch)
        } else {
            status_mutation_response(&editor_state)
        };
        (
            MutationExecution::new(response, result.search_index_work),
            result.retired,
        )
    };
    drop(retired);
    Ok(execution)
}

fn search_index_work_for_changes(
    editor_state: &crate::state::editor_state::EditorState,
    changes: &[SheetCellChange],
) -> SearchIndexWork {
    let updates = changes
        .iter()
        .map(|change| {
            let sheet = editor_state.file_data().sheets.get(change.sheet_index);
            let format = sheet.and_then(|sheet| sheet.cell_format_at(change.row, change.col));
            let display_text = sheet
                .map(|sheet| sheet.cell_display_text(change.row, change.col))
                .unwrap_or_else(|| change.value.to_display_string());
            SearchCellIndexUpdate {
                sheet_index: change.sheet_index,
                row: change.row,
                col: change.col,
                search_text: DisplayProjection::search_text(&change.value, format.as_ref()),
                display_text,
            }
        })
        .collect();
    SearchIndexWork::UpdateCells(updates)
}

fn mutation_handle(
    registry: &ActiveDocumentRepository,
    document_id: u64,
) -> Result<std::sync::Arc<crate::state::state::DocumentHandle>, AppError> {
    registry.mutation_handle(document_id)
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
    use crate::state::state::ActiveDocumentRepository;
    use crate::types::{
        CellFormatProjection, CellValue, EditorPatch, FileData, ReadOnlyRichProjection, SheetData,
    };
    use serde_json::Value;
    use std::collections::HashMap;

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

    fn make_formatted_registry() -> ActiveDocumentRepository {
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
                if patch.sheet_index == 0 && patch.row_index == 1 && patch.count == 1
        ));
        assert_eq!(add_row_response.patches.len(), 1);
        assert_eq!(
            add_row_response.search_index_work,
            SearchIndexWork::RebuildAll
        );

        let registry = make_registry();
        let (document_id, revision) = command_session(&registry);
        let add_column_response =
            do_add_column(&registry, document_id, revision, 0, 1).expect("add column");
        assert!(matches!(
            add_column_response.patches.first(),
            Some(EditorPatch::ColumnInserted { patch })
                if patch.sheet_index == 0 && patch.col_index == 1 && patch.count == 1
        ));
        assert_eq!(add_column_response.patches.len(), 1);
        assert_eq!(
            add_column_response.search_index_work,
            SearchIndexWork::RebuildAll
        );
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
    fn batched_duplicate_cell_edits_return_the_final_value_once() {
        let registry = make_registry();
        let (document_id, revision) = command_session(&registry);

        let response = do_set_cells(
            &registry,
            document_id,
            revision,
            vec![
                CellEditInput {
                    sheet_index: 0,
                    row: 0,
                    col: 0,
                    text: "first".to_string(),
                },
                CellEditInput {
                    sheet_index: 0,
                    row: 0,
                    col: 0,
                    text: "final".to_string(),
                },
            ],
        )
        .expect("batch edit");

        let Some(EditorPatch::Cells { changes }) = response.patches.first() else {
            panic!("expected cell patch");
        };
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].row, 0);
        assert_eq!(changes[0].col, 0);
        assert_eq!(changes[0].value, CellValue::String("final".to_string()));

        let handle = registry.active_handle().unwrap().unwrap();
        let editor = handle.read().unwrap();
        assert_eq!(
            editor.file_data().sheets[0].rows[0][0],
            CellValue::String("final".to_string())
        );
    }

    #[test]
    fn batched_duplicate_cell_edits_are_noop_when_final_value_matches_original() {
        let registry = make_registry();
        let (document_id, revision) = command_session(&registry);

        let response = do_set_cells(
            &registry,
            document_id,
            revision,
            vec![
                CellEditInput {
                    sheet_index: 0,
                    row: 0,
                    col: 0,
                    text: "draft".to_string(),
                },
                CellEditInput {
                    sheet_index: 0,
                    row: 0,
                    col: 0,
                    text: "A1".to_string(),
                },
            ],
        )
        .expect("batch edit");

        assert_eq!(response.revision, revision);
        assert!(response.patches.is_empty());
        let handle = registry.active_handle().unwrap().unwrap();
        let editor = handle.read().unwrap();
        assert_eq!(
            editor.file_data().sheets[0].rows[0][0],
            CellValue::String("A1".to_string())
        );
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
        let json = serde_json::to_value(response.response).expect("serialize response");

        assert!(json.get("searchIndexUpdate").is_none());
        assert_eq!(json["documentId"], document_id.to_string());
        assert_eq!(json["revision"], (revision + 1).to_string());
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
