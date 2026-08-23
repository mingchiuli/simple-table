use std::sync::Arc;

use crate::application::image_service::StagedImage;
use crate::application::mutation_intent::MutationIntent;
use crate::application::mutation_replay::{self, MutationReplayCoordinator};
use crate::application::search_ports::SearchIndexMaintenancePort;
use crate::domain::EditorCommand;
use crate::error::AppError;
use crate::ops::mutation_execution::MutationExecution;
use crate::ops::{cell_ops, editor_ops};
use crate::projection_model::MutationOutcome;
use crate::state::ActiveDocumentRepository;

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

pub fn insert_image_command(
    sheet_index: usize,
    row: u32,
    col: u32,
    staged: StagedImage,
) -> EditorCommand {
    const EMU_PER_PIXEL: i64 = 9_525;
    let scale = (320.0 / f64::from(staged.width))
        .min(240.0 / f64::from(staged.height))
        .min(1.0);
    let display_width = (f64::from(staged.width) * scale).round().max(1.0) as i64;
    let display_height = (f64::from(staged.height) * scale).round().max(1.0) as i64;
    let image_id = uuid::Uuid::new_v4().to_string();
    let image = crate::document_data::SheetImage {
        id: image_id,
        media_id: staged.media_id,
        mime_type: staged.mime_type,
        intrinsic_width: staged.width,
        intrinsic_height: staged.height,
        anchor: crate::document_data::ImageAnchor::OneCell {
            from: crate::document_data::ImageMarker {
                row,
                col,
                ..Default::default()
            },
            width_emu: display_width * EMU_PER_PIXEL,
            height_emu: display_height * EMU_PER_PIXEL,
        },
        z_index: 0,
        renderable: true,
    };
    EditorCommand::InsertImage {
        sheet_index,
        image,
        image_name: staged.file_name,
        bytes: staged.bytes,
    }
}

pub fn execute(
    service: &EditorCommandService,
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    command: EditorCommand,
) -> Result<Arc<MutationOutcome>, AppError> {
    run_mutation(
        service,
        document_id,
        base_revision,
        command_id,
        MutationIntent::Execute(command),
    )
}

pub fn undo(
    service: &EditorCommandService,
    document_id: u64,
    base_revision: u64,
    command_id: &str,
) -> Result<Arc<MutationOutcome>, AppError> {
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
) -> Result<Arc<MutationOutcome>, AppError> {
    run_mutation(
        service,
        document_id,
        base_revision,
        command_id,
        MutationIntent::Redo,
    )
}

fn run_mutation(
    service: &EditorCommandService,
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    intent: MutationIntent,
) -> Result<Arc<MutationOutcome>, AppError> {
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
