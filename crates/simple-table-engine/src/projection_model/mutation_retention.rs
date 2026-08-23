use crate::domain::CellValue;
use std::sync::Arc;

use super::mutation::{MutationOutcome, MutationPatch};

pub(crate) struct MutationReplayPayload {
    pub(crate) outcome: Arc<MutationOutcome>,
    pub(crate) retained_bytes: usize,
}

pub(crate) fn prepare_mutation_replay_payload(
    outcome: &Arc<MutationOutcome>,
    maximum_bytes: usize,
) -> Option<MutationReplayPayload> {
    let original_bytes = estimated_mutation_outcome_bytes(outcome);
    let (outcome, retained_bytes) = if original_bytes <= maximum_bytes {
        (Arc::clone(outcome), original_bytes)
    } else {
        let mut compact = (**outcome).clone();
        compact.require_resync("mutation response exceeded replay budget");
        let compact_bytes = estimated_mutation_outcome_bytes(&compact);
        (Arc::new(compact), compact_bytes)
    };

    (retained_bytes <= maximum_bytes).then_some(MutationReplayPayload {
        outcome,
        retained_bytes,
    })
}

fn estimated_mutation_outcome_bytes(outcome: &MutationOutcome) -> usize {
    let patch_bytes = outcome
        .patches
        .iter()
        .map(|patch| match patch {
            MutationPatch::Cells { changes } => changes
                .iter()
                .map(|change| {
                    96usize
                        .saturating_add(change.display.as_ref().map_or(0, String::len))
                        .saturating_add(estimated_cell_value_bytes(&change.value))
                })
                .sum(),
            MutationPatch::SheetInserted { sheet, .. } => sheet.name.len().saturating_mul(6) + 256,
            MutationPatch::SheetsReplaced { sheets, .. } => sheets
                .iter()
                .map(|sheet| sheet.name.len().saturating_mul(6) + 256)
                .sum(),
            MutationPatch::ResyncRequired { reason } => reason.len().saturating_mul(6) + 64,
            MutationPatch::Layout {
                column_widths,
                row_heights,
                ..
            } => column_widths
                .len()
                .saturating_add(row_heights.len())
                .saturating_mul(48),
            MutationPatch::SheetDeleted { .. }
            | MutationPatch::SheetInvalidated { .. }
            | MutationPatch::RowInserted { .. }
            | MutationPatch::RowDeleted { .. }
            | MutationPatch::ColumnInserted { .. }
            | MutationPatch::ColumnDeleted { .. } => 96,
            MutationPatch::ImageUpserted { image, .. } => {
                image.id.len() + image.media_id.len() + image.mime_type.len() + 256
            }
            MutationPatch::ImageDeleted { image_id, .. } => image_id.len() + 96,
        })
        .sum::<usize>();
    std::mem::size_of::<MutationOutcome>()
        .saturating_add(patch_bytes)
        .saturating_add(2048)
}

fn estimated_cell_value_bytes(value: &CellValue) -> usize {
    match value {
        CellValue::Null => 8,
        CellValue::String(value) => value.len().saturating_mul(6).saturating_add(16),
        CellValue::Number(_) | CellValue::Boolean(_) => 32,
        CellValue::Formula {
            formula,
            cached_value,
            error,
        } => formula
            .len()
            .saturating_mul(6)
            .saturating_add(estimated_cell_value_bytes(cached_value))
            .saturating_add(
                error
                    .as_ref()
                    .map_or(0, |value| value.len().saturating_mul(6)),
            )
            .saturating_add(64),
    }
}
