use std::sync::Arc;

use crate::application::mutation_intent::MutationIntent;
use crate::application::mutation_replay::{self, MutationReplayCoordinator};
use crate::application::search_ports::SearchIndexMaintenancePort;
use crate::domain::{CellEditInput, EditorCommand};
use crate::error::AppError;
use crate::ops::mutation_execution::MutationExecution;
use crate::ops::{cell_ops, editor_ops};
use crate::projection_model::{EditorSessionSnapshot, MutationLookup, MutationOutcome};
use crate::state::state::ActiveDocumentRepository;

#[derive(Clone)]
pub struct EditorCommandService {
    documents: ActiveDocumentRepository,
    mutation_replays: Arc<MutationReplayCoordinator>,
    search_indexes: Arc<dyn SearchIndexMaintenancePort>,
}

impl EditorCommandService {
    pub(crate) fn new(
        documents: ActiveDocumentRepository,
        mutation_replays: Arc<MutationReplayCoordinator>,
        search_indexes: Arc<dyn SearchIndexMaintenancePort>,
    ) -> Self {
        Self {
            documents,
            mutation_replays,
            search_indexes,
        }
    }

    fn documents(&self) -> &ActiveDocumentRepository {
        &self.documents
    }

    fn mutation_replays(&self) -> &Arc<MutationReplayCoordinator> {
        &self.mutation_replays
    }

    fn search_indexes(&self) -> &dyn SearchIndexMaintenancePort {
        self.search_indexes.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn is_isolated_from(&self, other: &Self) -> bool {
        !self.documents.is_same_instance(&other.documents)
            && !Arc::ptr_eq(&self.mutation_replays, &other.mutation_replays)
            && !Arc::ptr_eq(&self.search_indexes, &other.search_indexes)
    }
}

pub fn get_editor_state(
    service: &EditorCommandService,
    document_id: Option<u64>,
    base_revision: Option<u64>,
) -> Result<Option<EditorSessionSnapshot>, AppError> {
    editor_ops::do_get_editor_state(service.documents(), document_id, base_revision)
}

pub fn get_mutation_result(
    service: &EditorCommandService,
    document_id: u64,
    command_id: &str,
) -> Result<MutationLookup, AppError> {
    mutation_replay::get(service.mutation_replays(), document_id, command_id)
}

pub fn undo(
    service: &EditorCommandService,
    document_id: u64,
    base_revision: u64,
    command_id: &str,
) -> Result<MutationOutcome, AppError> {
    run_mutation(
        service,
        document_id,
        base_revision,
        command_id,
        MutationIntent::Undo,
    )
}

pub fn redo(
    service: &EditorCommandService,
    document_id: u64,
    base_revision: u64,
    command_id: &str,
) -> Result<MutationOutcome, AppError> {
    run_mutation(
        service,
        document_id,
        base_revision,
        command_id,
        MutationIntent::Redo,
    )
}

pub fn set_cell(
    service: &EditorCommandService,
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    sheet_index: usize,
    row: usize,
    col: usize,
    text: String,
) -> Result<MutationOutcome, AppError> {
    run_editor_command(
        service,
        document_id,
        base_revision,
        command_id,
        EditorCommand::SetCell {
            sheet_index,
            row,
            col,
            text,
        },
    )
}

pub fn set_cells(
    service: &EditorCommandService,
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    edits: Vec<CellEditInput>,
) -> Result<MutationOutcome, AppError> {
    run_editor_command(
        service,
        document_id,
        base_revision,
        command_id,
        EditorCommand::SetCells { changes: edits },
    )
}

pub fn add_row(
    service: &EditorCommandService,
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    sheet_index: usize,
    row_index: usize,
) -> Result<MutationOutcome, AppError> {
    run_editor_command(
        service,
        document_id,
        base_revision,
        command_id,
        EditorCommand::AddRow {
            sheet_index,
            row_index,
        },
    )
}

pub fn delete_row(
    service: &EditorCommandService,
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    sheet_index: usize,
    row_index: usize,
) -> Result<MutationOutcome, AppError> {
    run_editor_command(
        service,
        document_id,
        base_revision,
        command_id,
        EditorCommand::DeleteRow {
            sheet_index,
            row_index,
        },
    )
}

pub fn add_column(
    service: &EditorCommandService,
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    sheet_index: usize,
    col_index: usize,
) -> Result<MutationOutcome, AppError> {
    run_editor_command(
        service,
        document_id,
        base_revision,
        command_id,
        EditorCommand::AddColumn {
            sheet_index,
            col_index,
        },
    )
}

pub fn delete_column(
    service: &EditorCommandService,
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    sheet_index: usize,
    col_index: usize,
) -> Result<MutationOutcome, AppError> {
    run_editor_command(
        service,
        document_id,
        base_revision,
        command_id,
        EditorCommand::DeleteColumn {
            sheet_index,
            col_index,
        },
    )
}

pub fn set_column_width(
    service: &EditorCommandService,
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    sheet_index: usize,
    col_index: usize,
    width: Option<u32>,
) -> Result<MutationOutcome, AppError> {
    run_editor_command(
        service,
        document_id,
        base_revision,
        command_id,
        EditorCommand::SetColumnWidth {
            sheet_index,
            col_index,
            width,
        },
    )
}

pub fn set_row_height(
    service: &EditorCommandService,
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    sheet_index: usize,
    row_index: usize,
    height: Option<u32>,
) -> Result<MutationOutcome, AppError> {
    run_editor_command(
        service,
        document_id,
        base_revision,
        command_id,
        EditorCommand::SetRowHeight {
            sheet_index,
            row_index,
            height,
        },
    )
}

pub fn add_sheet(
    service: &EditorCommandService,
    document_id: u64,
    base_revision: u64,
    command_id: &str,
) -> Result<MutationOutcome, AppError> {
    run_editor_command(
        service,
        document_id,
        base_revision,
        command_id,
        EditorCommand::AddSheet { name: None },
    )
}

pub fn delete_sheet(
    service: &EditorCommandService,
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    sheet_index: usize,
) -> Result<MutationOutcome, AppError> {
    run_editor_command(
        service,
        document_id,
        base_revision,
        command_id,
        EditorCommand::DeleteSheet { sheet_index },
    )
}

fn run_editor_command(
    service: &EditorCommandService,
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    command: EditorCommand,
) -> Result<MutationOutcome, AppError> {
    run_mutation(
        service,
        document_id,
        base_revision,
        command_id,
        MutationIntent::Execute(command),
    )
}

fn run_mutation(
    service: &EditorCommandService,
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    intent: MutationIntent,
) -> Result<MutationOutcome, AppError> {
    mutation_replay::run(
        service.mutation_replays(),
        document_id,
        base_revision,
        command_id,
        intent,
        |intent| {
            let execution =
                execute_intent(service.documents(), document_id, base_revision, intent)?;
            let revision = execution.outcome.revision;
            service.search_indexes().schedule_work(
                document_id,
                revision,
                execution.search_index_work,
            );
            Ok(execution.outcome)
        },
    )
}

fn execute_intent(
    documents: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
    intent: MutationIntent,
) -> Result<MutationExecution, AppError> {
    match intent {
        MutationIntent::Undo => editor_ops::do_undo(documents, document_id, base_revision),
        MutationIntent::Redo => editor_ops::do_redo(documents, document_id, base_revision),
        MutationIntent::Execute(command) => {
            cell_ops::do_execute_command(documents, document_id, base_revision, command)
        }
    }
}
