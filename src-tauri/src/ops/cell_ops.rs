use std::sync::{Arc, RwLock};

use crate::error::AppError;
use crate::ops::EditorCommand;
use crate::ops::editor_ops::{
    cell_delta_mutation_response, layout_mutation_response, snapshot_mutation_response,
    structural_delta_mutation_response,
};
use crate::ops::index_ops::schedule_index_for_response;
use crate::state::state::ActiveDocumentStore;
use crate::types::{EditorMutationResponse, LayoutPatch, SetCellRequest};

pub fn do_set_cell(
    registry: Arc<RwLock<ActiveDocumentStore>>,
    sheet_index: usize,
    row: usize,
    col: usize,
    text: String,
) -> Result<EditorMutationResponse, AppError> {
    let response = execute_cell_delta(
        registry.clone(),
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
    registry: Arc<RwLock<ActiveDocumentStore>>,
    changes: Vec<SetCellRequest>,
) -> Result<EditorMutationResponse, AppError> {
    let response = execute_cell_delta(registry.clone(), EditorCommand::SetCells { changes });

    if let Ok(response) = &response {
        schedule_index_for_response(response, registry);
    }

    response
}

pub fn do_add_row(
    registry: Arc<RwLock<ActiveDocumentStore>>,
    sheet_index: usize,
    row_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    execute_sheet_snapshot(
        registry,
        EditorCommand::AddRow {
            sheet_index,
            row_index,
        },
        sheet_index,
    )
}

pub fn do_delete_row(
    registry: Arc<RwLock<ActiveDocumentStore>>,
    sheet_index: usize,
    row_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    execute_sheet_snapshot(
        registry,
        EditorCommand::DeleteRow {
            sheet_index,
            row_index,
        },
        sheet_index,
    )
}

pub fn do_add_column(
    registry: Arc<RwLock<ActiveDocumentStore>>,
    sheet_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    execute_sheet_snapshot(
        registry,
        EditorCommand::AddColumn { sheet_index },
        sheet_index,
    )
}

pub fn do_delete_column(
    registry: Arc<RwLock<ActiveDocumentStore>>,
    sheet_index: usize,
    col_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    execute_sheet_snapshot(
        registry,
        EditorCommand::DeleteColumn {
            sheet_index,
            col_index,
        },
        sheet_index,
    )
}

pub fn do_set_column_width(
    registry: Arc<RwLock<ActiveDocumentStore>>,
    sheet_index: usize,
    col_index: usize,
    width: Option<u32>,
) -> Result<EditorMutationResponse, AppError> {
    execute_layout(
        registry,
        EditorCommand::SetColumnWidth {
            sheet_index,
            col_index,
            width,
        },
        column_width_patch(sheet_index, col_index, width),
    )
}

pub fn do_set_row_height(
    registry: Arc<RwLock<ActiveDocumentStore>>,
    sheet_index: usize,
    row_index: usize,
    height: Option<u32>,
) -> Result<EditorMutationResponse, AppError> {
    execute_layout(
        registry,
        EditorCommand::SetRowHeight {
            sheet_index,
            row_index,
            height,
        },
        row_height_patch(sheet_index, row_index, height),
    )
}

pub fn do_add_sheet(
    registry: Arc<RwLock<ActiveDocumentStore>>,
) -> Result<EditorMutationResponse, AppError> {
    execute_structural_snapshot(registry, EditorCommand::AddSheet { name: None })
}

pub fn do_delete_sheet(
    registry: Arc<RwLock<ActiveDocumentStore>>,
    sheet_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    execute_structural_snapshot(registry, EditorCommand::DeleteSheet { sheet_index })
}

fn execute_cell_delta(
    registry: Arc<RwLock<ActiveDocumentStore>>,
    command: EditorCommand,
) -> Result<EditorMutationResponse, AppError> {
    let mut registry_guard = registry.write().expect("Document registry lock poisoned");
    let editor_state = registry_guard.active_mut().ok_or(AppError::NoFileLoaded)?;
    let result = editor_state.execute(command)?;
    if let Some(operation) = result.operation {
        Ok(cell_delta_mutation_response(
            editor_state,
            operation,
            result.cell_changes,
        ))
    } else {
        Ok(snapshot_mutation_response(editor_state, None))
    }
}

fn execute_structural_snapshot(
    registry: Arc<RwLock<ActiveDocumentStore>>,
    command: EditorCommand,
) -> Result<EditorMutationResponse, AppError> {
    let response = {
        let mut registry_guard = registry.write().expect("Document registry lock poisoned");
        let editor_state = registry_guard.active_mut().ok_or(AppError::NoFileLoaded)?;
        let result = editor_state.execute(command)?;
        match result.operation {
            Some(operation) => {
                structural_delta_mutation_response(editor_state, operation, result.cell_changes)
            }
            None => snapshot_mutation_response(editor_state, None),
        }
    };

    schedule_index_for_response(&response, registry);

    Ok(response)
}

fn execute_sheet_snapshot(
    registry: Arc<RwLock<ActiveDocumentStore>>,
    command: EditorCommand,
    _sheet_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    let response = {
        let mut registry_guard = registry.write().expect("Document registry lock poisoned");
        let editor_state = registry_guard.active_mut().ok_or(AppError::NoFileLoaded)?;
        let result = editor_state.execute(command)?;
        match result.operation {
            Some(operation) => {
                structural_delta_mutation_response(editor_state, operation, result.cell_changes)
            }
            None => snapshot_mutation_response(editor_state, None),
        }
    };

    schedule_index_for_response(&response, registry);

    Ok(response)
}

fn execute_layout(
    registry: Arc<RwLock<ActiveDocumentStore>>,
    command: EditorCommand,
    patch: LayoutPatch,
) -> Result<EditorMutationResponse, AppError> {
    let mut registry_guard = registry.write().expect("Document registry lock poisoned");
    let editor_state = registry_guard.active_mut().ok_or(AppError::NoFileLoaded)?;
    let _result = editor_state.execute(command)?;
    Ok(layout_mutation_response(editor_state, patch))
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
    use crate::types::{CellValue, EditorPatch, FileData, SheetData};

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

    #[test]
    fn row_and_column_structure_edits_return_local_structure_patches() {
        let add_row_response = do_add_row(make_registry(), 0, 1).expect("add row");
        assert!(matches!(
            add_row_response.patches.first(),
            Some(EditorPatch::RowInserted { patch }) if patch.sheet_index == 0 && patch.row_index == 1
        ));

        let add_column_response = do_add_column(make_registry(), 0).expect("add column");
        assert!(matches!(
            add_column_response.patches.first(),
            Some(EditorPatch::ColumnInserted { patch }) if patch.sheet_index == 0 && patch.column_index == 1
        ));
    }
}
