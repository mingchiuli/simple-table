use serde::Serialize;

use crate::application::mutation_replay;
use crate::error::AppError;
use crate::ops::{cell_ops, editor_ops, search_ops};
use crate::state::{active_document_store, state::ActiveDocumentStore};
use crate::types::{
    EditorMutationResponse, MutationResultLookup, SearchResponse, SearchScope, SetCellRequest,
};

pub use crate::state::state::EditorSessionInfo;

pub fn get_editor_state(
    document_id: Option<u64>,
    base_revision: Option<u64>,
) -> Result<Option<EditorSessionInfo>, AppError> {
    editor_ops::do_get_editor_state(&active_document_store(), document_id, base_revision)
}

pub fn get_mutation_result(
    document_id: u64,
    command_id: &str,
) -> Result<MutationResultLookup, AppError> {
    mutation_replay::get(document_id, command_id)
}

pub fn undo(
    document_id: u64,
    base_revision: u64,
    command_id: &str,
) -> Result<EditorMutationResponse, AppError> {
    run_mutation(
        document_id,
        base_revision,
        command_id,
        "undo",
        &(),
        |registry| editor_ops::do_undo(registry, document_id, base_revision),
    )
}

pub fn redo(
    document_id: u64,
    base_revision: u64,
    command_id: &str,
) -> Result<EditorMutationResponse, AppError> {
    run_mutation(
        document_id,
        base_revision,
        command_id,
        "redo",
        &(),
        |registry| editor_ops::do_redo(registry, document_id, base_revision),
    )
}

pub fn set_cell(
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    sheet_index: usize,
    row: usize,
    col: usize,
    text: String,
) -> Result<EditorMutationResponse, AppError> {
    let payload = (sheet_index, row, col, &text);
    run_mutation(
        document_id,
        base_revision,
        command_id,
        "set_cell",
        &payload,
        |registry| {
            cell_ops::do_set_cell(
                registry,
                document_id,
                base_revision,
                sheet_index,
                row,
                col,
                text.clone(),
            )
        },
    )
}

pub fn set_cells(
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    changes: Vec<SetCellRequest>,
) -> Result<EditorMutationResponse, AppError> {
    run_mutation(
        document_id,
        base_revision,
        command_id,
        "set_cells",
        &changes,
        |registry| cell_ops::do_set_cells(registry, document_id, base_revision, changes.clone()),
    )
}

pub fn add_row(
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    sheet_index: usize,
    row_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    run_mutation(
        document_id,
        base_revision,
        command_id,
        "add_row",
        &(sheet_index, row_index),
        |registry| {
            cell_ops::do_add_row(registry, document_id, base_revision, sheet_index, row_index)
        },
    )
}

pub fn delete_row(
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    sheet_index: usize,
    row_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    run_mutation(
        document_id,
        base_revision,
        command_id,
        "delete_row",
        &(sheet_index, row_index),
        |registry| {
            cell_ops::do_delete_row(registry, document_id, base_revision, sheet_index, row_index)
        },
    )
}

pub fn add_column(
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    sheet_index: usize,
    col_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    run_mutation(
        document_id,
        base_revision,
        command_id,
        "add_column",
        &(sheet_index, col_index),
        |registry| {
            cell_ops::do_add_column(registry, document_id, base_revision, sheet_index, col_index)
        },
    )
}

pub fn delete_column(
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    sheet_index: usize,
    col_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    run_mutation(
        document_id,
        base_revision,
        command_id,
        "delete_column",
        &(sheet_index, col_index),
        |registry| {
            cell_ops::do_delete_column(registry, document_id, base_revision, sheet_index, col_index)
        },
    )
}

pub fn set_column_width(
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    sheet_index: usize,
    col_index: usize,
    width: Option<u32>,
) -> Result<EditorMutationResponse, AppError> {
    run_mutation(
        document_id,
        base_revision,
        command_id,
        "set_column_width",
        &(sheet_index, col_index, width),
        |registry| {
            cell_ops::do_set_column_width(
                registry,
                document_id,
                base_revision,
                sheet_index,
                col_index,
                width,
            )
        },
    )
}

pub fn set_row_height(
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    sheet_index: usize,
    row_index: usize,
    height: Option<u32>,
) -> Result<EditorMutationResponse, AppError> {
    run_mutation(
        document_id,
        base_revision,
        command_id,
        "set_row_height",
        &(sheet_index, row_index, height),
        |registry| {
            cell_ops::do_set_row_height(
                registry,
                document_id,
                base_revision,
                sheet_index,
                row_index,
                height,
            )
        },
    )
}

pub fn add_sheet(
    document_id: u64,
    base_revision: u64,
    command_id: &str,
) -> Result<EditorMutationResponse, AppError> {
    run_mutation(
        document_id,
        base_revision,
        command_id,
        "add_sheet",
        &(),
        |registry| cell_ops::do_add_sheet(registry, document_id, base_revision),
    )
}

pub fn delete_sheet(
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    sheet_index: usize,
) -> Result<EditorMutationResponse, AppError> {
    run_mutation(
        document_id,
        base_revision,
        command_id,
        "delete_sheet",
        &sheet_index,
        |registry| cell_ops::do_delete_sheet(registry, document_id, base_revision, sheet_index),
    )
}

pub fn search(
    document_id: u64,
    base_revision: u64,
    query: &str,
    scope: SearchScope,
    current_sheet_index: Option<usize>,
) -> Result<SearchResponse, AppError> {
    search_ops::do_search(
        &active_document_store(),
        document_id,
        base_revision,
        query,
        scope,
        current_sheet_index,
    )
}

fn run_mutation<P: Serialize>(
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    command_name: &str,
    payload: &P,
    execute: impl FnOnce(
        &std::sync::Arc<std::sync::RwLock<ActiveDocumentStore>>,
    ) -> Result<EditorMutationResponse, AppError>,
) -> Result<EditorMutationResponse, AppError> {
    let registry = active_document_store();
    mutation_replay::run(
        document_id,
        base_revision,
        command_id,
        command_name,
        payload,
        || execute(&registry),
    )
}
