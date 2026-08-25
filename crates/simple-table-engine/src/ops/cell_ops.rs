use crate::document_data::ImageAnchor;
#[cfg(test)]
use crate::domain::CellEditInput;
use crate::domain::EditorCommand;
use crate::domain::{AppliedOperation, SearchCellIndexUpdate, SearchIndexWork};
use crate::error::AppError;
use crate::ops::mutation_execution::MutationExecution;
use crate::ops::patch_projector::{
    cell_delta_mutation_outcome, complete_cell_changes, layout_mutation_outcome,
    resync_required_mutation_outcome, status_mutation_outcome, structural_delta_mutation_outcome,
};
use crate::projection_model::{MutationPatch, ProjectedCellChange};
use crate::state::ActiveDocumentRepository;
use std::collections::HashMap;

struct LayoutMutation {
    sheet_index: usize,
    column_widths: HashMap<usize, Option<u32>>,
    row_heights: HashMap<usize, Option<u32>>,
}

pub fn do_execute_command(
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
    command: EditorCommand,
) -> Result<MutationExecution, AppError> {
    match &command {
        EditorCommand::SetFilter { .. } | EditorCommand::ClearFilter { .. } => {
            execute_filter_command(registry, document_id, base_revision, command)
        }
        EditorCommand::SetCell { .. } | EditorCommand::SetCells { .. } => {
            execute_cell_delta(registry, document_id, base_revision, command)
        }
        EditorCommand::SetColumnWidth {
            sheet_index,
            col_index,
            width,
        } => {
            let patch = column_width_patch(*sheet_index, *col_index, *width);
            execute_layout(registry, document_id, base_revision, command, patch)
        }
        EditorCommand::SetRowHeight {
            sheet_index,
            row_index,
            height,
        } => {
            let patch = row_height_patch(*sheet_index, *row_index, *height);
            execute_layout(registry, document_id, base_revision, command, patch)
        }
        EditorCommand::AddRow { .. }
        | EditorCommand::DeleteRow { .. }
        | EditorCommand::AddColumn { .. }
        | EditorCommand::DeleteColumn { .. }
        | EditorCommand::AddSheet { .. }
        | EditorCommand::DeleteSheet { .. }
        | EditorCommand::SortRows { .. } => {
            execute_structural_command(registry, document_id, base_revision, command)
        }
        EditorCommand::InsertImage { .. }
        | EditorCommand::UpdateImage { .. }
        | EditorCommand::DeleteImage { .. } => {
            execute_image_command(registry, document_id, base_revision, command)
        }
    }
}

fn execute_filter_command(
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
    command: EditorCommand,
) -> Result<MutationExecution, AppError> {
    let handle = mutation_handle(registry, document_id)?;
    let (execution, retired) = {
        let mut editor_state = handle.write_for_command(document_id, base_revision)?;
        let result = match command {
            EditorCommand::SetFilter {
                sheet_index,
                anchor_row,
                col,
                operator,
                value,
            } => editor_state.set_filter(sheet_index, anchor_row, col, operator, value)?,
            EditorCommand::ClearFilter { sheet_index, col } => {
                editor_state.clear_filter(sheet_index, col)?
            }
            _ => unreachable!("filter executor receives only filter commands"),
        };
        let execution = MutationExecution::new(
            crate::ops::patch_projector::restore_mutation_outcome(&editor_state, result.restore),
            result.search_index_work,
        );
        (execution, result.retired)
    };
    drop(retired);
    Ok(execution)
}

fn execute_image_command(
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
    command: EditorCommand,
) -> Result<MutationExecution, AppError> {
    let handle = mutation_handle(registry, document_id)?;
    let (execution, retired) = {
        let mut editor_state = handle.write_for_command(document_id, base_revision)?;
        let result = editor_state.execute(command)?;
        let execution = match result.operation {
            Some(operation) => {
                let projected = operation
                    .patch_projector()
                    .projected_result_from_current_file(editor_state.file_data());
                let mut outcome = structural_delta_mutation_outcome(
                    &editor_state,
                    &projected,
                    result.cell_changes,
                );
                if let Some(layout_patch) = insert_image_layout_patch(&operation) {
                    outcome.patches.push(layout_patch);
                }
                MutationExecution::new(outcome, result.search_index_work)
            }
            None => MutationExecution::new(
                status_mutation_outcome(&editor_state),
                SearchIndexWork::None,
            ),
        };
        (execution, result.retired)
    };
    drop(retired);
    Ok(execution)
}

/// For an image insert that resizes its containing cell, build the `Layout`
/// patch so the frontend refetches the document and renders the new sizes.
fn insert_image_layout_patch(operation: &AppliedOperation) -> Option<MutationPatch> {
    match operation {
        AppliedOperation::InsertImage {
            sheet_index,
            image,
            column_width,
            row_height,
            ..
        } => {
            let ImageAnchor::OneCell { from, .. } = &image.anchor else {
                return None;
            };
            let mut column_widths = HashMap::new();
            let mut row_heights = HashMap::new();
            if let Some(width) = column_width {
                column_widths.insert(from.col as usize, Some(*width));
            }
            if let Some(height) = row_height {
                row_heights.insert(from.row as usize, Some(*height));
            }
            if column_widths.is_empty() && row_heights.is_empty() {
                return None;
            }
            Some(MutationPatch::Layout {
                sheet_index: *sheet_index,
                column_widths,
                row_heights,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
pub fn do_set_cell(
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    row: usize,
    col: usize,
    text: String,
) -> Result<MutationExecution, AppError> {
    execute_cell_delta(
        registry,
        document_id,
        base_revision,
        EditorCommand::SetCell {
            sheet_index,
            row,
            col,
            text,
        },
    )
}

#[cfg(test)]
pub fn do_set_cells(
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
    changes: Vec<CellEditInput>,
) -> Result<MutationExecution, AppError> {
    execute_cell_delta(
        registry,
        document_id,
        base_revision,
        EditorCommand::SetCells { changes },
    )
}

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
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
            let projected = operation
                .patch_projector()
                .projected_result_from_current_file(editor_state.file_data());
            let changes = complete_cell_changes(&projected, result.cell_changes);
            let search_index_work = search_index_work_for_changes(&editor_state, &changes);
            MutationExecution::new(
                cell_delta_mutation_outcome(&editor_state, changes),
                search_index_work,
            )
        } else {
            MutationExecution::new(
                status_mutation_outcome(&editor_state),
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
            Some(operation) => {
                let projected = operation
                    .patch_projector()
                    .projected_result_from_current_file(editor_state.file_data());
                MutationExecution::new(
                    structural_delta_mutation_outcome(
                        &editor_state,
                        &projected,
                        result.cell_changes,
                    ),
                    result.search_index_work,
                )
            }
            None => MutationExecution::new(
                resync_required_mutation_outcome(
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
    patch: LayoutMutation,
) -> Result<MutationExecution, AppError> {
    let handle = mutation_handle(registry, document_id)?;
    let (execution, retired) = {
        let mut editor_state = handle.write_for_command(document_id, base_revision)?;
        let result = editor_state.execute(command)?;
        let response = if result.operation.is_some() {
            layout_mutation_outcome(
                &editor_state,
                patch.sheet_index,
                patch.column_widths,
                patch.row_heights,
            )
        } else {
            status_mutation_outcome(&editor_state)
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
    changes: &[ProjectedCellChange],
) -> SearchIndexWork {
    let updates = changes
        .iter()
        .map(|change| {
            let sheet = editor_state.file_data().sheets.get(change.sheet_index);
            let display_text = sheet
                .map(|sheet| sheet.cell_display_text(change.row, change.col))
                .unwrap_or_else(|| change.value.to_display_string());
            SearchCellIndexUpdate {
                sheet_index: change.sheet_index,
                row: change.row,
                col: change.col,
                search_text: sheet
                    .map(|sheet| sheet.cell_search_text(change.row, change.col))
                    .unwrap_or_else(|| change.value.to_display_string()),
                display_text,
            }
        })
        .collect();
    SearchIndexWork::UpdateCells(updates)
}

fn mutation_handle(
    registry: &ActiveDocumentRepository,
    document_id: u64,
) -> Result<std::sync::Arc<crate::state::DocumentHandle>, AppError> {
    registry.mutation_handle(document_id)
}

fn column_width_patch(sheet_index: usize, col_index: usize, width: Option<u32>) -> LayoutMutation {
    LayoutMutation {
        sheet_index,
        column_widths: [(col_index, width)].into_iter().collect(),
        row_heights: Default::default(),
    }
}

fn row_height_patch(sheet_index: usize, row_index: usize, height: Option<u32>) -> LayoutMutation {
    LayoutMutation {
        sheet_index,
        column_widths: Default::default(),
        row_heights: [(row_index, height)].into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_data::{CellFormat, DocumentData, DocumentSheet, RichMetadata};
    use crate::domain::{CellNumber, CellValue};
    use crate::projection_model::MutationPatch;
    use crate::state::ActiveDocumentRepository;
    use crate::state::editor_state::EditorState;
    use std::collections::HashMap;

    fn make_registry() -> ActiveDocumentRepository {
        let editor = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "test.xlsx".to_string(),
                sheets: vec![DocumentSheet {
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
            DocumentData {
                path: String::new(),
                file_name: "test.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::Number(CellNumber::from_f64(0.4).unwrap())]],
                    rich: RichMetadata {
                        cell_formats: HashMap::from([(
                            "A1".to_string(),
                            CellFormat {
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
            add_row_response.outcome.patches.first(),
            Some(MutationPatch::RowInserted { sheet_index, row_index, count })
                if *sheet_index == 0 && *row_index == 1 && *count == 1
        ));
        assert_eq!(add_row_response.outcome.patches.len(), 1);
        assert_eq!(
            add_row_response.search_index_work,
            SearchIndexWork::RebuildAll
        );

        let registry = make_registry();
        let (document_id, revision) = command_session(&registry);
        let add_column_response =
            do_add_column(&registry, document_id, revision, 0, 1).expect("add column");
        assert!(matches!(
            add_column_response.outcome.patches.first(),
            Some(MutationPatch::ColumnInserted { sheet_index, col_index, count })
                if *sheet_index == 0 && *col_index == 1 && *count == 1
        ));
        assert_eq!(add_column_response.outcome.patches.len(), 1);
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

        assert_eq!(response.outcome.revision, 0);
        assert!(response.outcome.patches.is_empty());
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

        let Some(MutationPatch::Cells { changes }) = response.outcome.patches.first() else {
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

        assert_eq!(response.outcome.revision, revision);
        assert!(response.outcome.patches.is_empty());
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

        assert_eq!(response.outcome.revision, 0);
        assert!(response.outcome.patches.is_empty());
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
                .outcome
                .patches
                .iter()
                .any(|patch| matches!(patch, MutationPatch::ResyncRequired { .. }))
        );
        assert!(
            response
                .outcome
                .patches
                .iter()
                .any(|patch| matches!(patch, MutationPatch::Cells { .. }))
        );
    }

    #[test]
    fn cell_delta_serializes_formatted_display_projection() {
        let registry = make_formatted_registry();
        let (document_id, revision) = command_session(&registry);
        let response = do_set_cell(&registry, document_id, revision, 0, 0, 0, "0.5".to_string())
            .expect("set formatted cell");
        let json = serde_json::to_value(crate::protocol_projection::mutation_response(
            &response.outcome,
        ))
        .expect("serialize response");

        assert!(json.get("searchIndexUpdate").is_none());
        assert_eq!(json["documentId"], document_id.to_string());
        assert_eq!(json["revision"], (revision + 1).to_string());
        assert_eq!(
            json["patches"][0]["data"]["changes"][0]["displayText"],
            "50%"
        );
        assert_eq!(json["patches"][0]["data"]["changes"][0]["editText"], "0.5");
    }
}
