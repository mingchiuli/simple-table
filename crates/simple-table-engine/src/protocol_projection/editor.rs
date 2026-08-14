use crate::projection_model::{MutationOutcome, MutationPatch};
use crate::resource_limits::MAX_MUTATION_RESPONSE_BYTES;
use crate::types;

use super::cell::{projected_cell_change, sheet_extent, sheet_manifest};
use super::size::serialized_json_bytes;
use super::status::{editor_state, formula_status, workbook_capabilities};

pub(crate) fn mutation_response(value: MutationOutcome) -> types::EditorMutationResponse {
    let mut response = types::EditorMutationResponse {
        protocol_version: types::EDITOR_MUTATION_PROTOCOL_VERSION,
        document_id: value.document_id,
        revision: value.revision,
        formula_status: formula_status(value.session.formula_status, 100),
        capabilities: workbook_capabilities(value.session.capabilities),
        editor_state: editor_state(value.session.editor_state),
        patches: value.patches.into_iter().map(mutation_patch).collect(),
        sheet_extents: value
            .sheet_extents
            .map(|extents| extents.into_iter().map(sheet_extent).collect()),
    };
    if serialized_json_bytes(&response).is_ok_and(|bytes| bytes <= MAX_MUTATION_RESPONSE_BYTES) {
        return response;
    }
    response.patches = vec![types::EditorPatch::ResyncRequired {
        patch: types::ResyncRequiredPatch {
            reason: "mutation response exceeded the response byte limit".to_string(),
        },
    }];
    response
}

fn mutation_patch(value: MutationPatch) -> types::EditorPatch {
    match value {
        MutationPatch::Cells { changes } => types::EditorPatch::Cells {
            changes: changes.into_iter().map(projected_cell_change).collect(),
        },
        MutationPatch::Layout {
            sheet_index,
            column_widths,
            row_heights,
        } => types::EditorPatch::Layout {
            patch: types::LayoutPatch {
                sheet_index,
                column_widths,
                row_heights,
            },
        },
        MutationPatch::SheetInserted { sheet_index, sheet } => types::EditorPatch::SheetInserted {
            patch: types::SheetInsertedPatch {
                sheet_index,
                sheet: sheet_manifest(sheet),
            },
        },
        MutationPatch::SheetDeleted { sheet_index } => types::EditorPatch::SheetDeleted {
            patch: types::SheetDeletedPatch { sheet_index },
        },
        MutationPatch::SheetInvalidated { sheet_index } => types::EditorPatch::SheetInvalidated {
            patch: types::SheetInvalidatedPatch { sheet_index },
        },
        MutationPatch::SheetsReplaced {
            start_index,
            sheets,
        } => types::EditorPatch::SheetsReplaced {
            patch: types::SheetsReplacedPatch {
                start_index,
                sheets: sheets.into_iter().map(sheet_manifest).collect(),
            },
        },
        MutationPatch::RowInserted {
            sheet_index,
            row_index,
            count,
        } => types::EditorPatch::RowInserted {
            patch: types::RowInsertedPatch {
                sheet_index,
                row_index,
                count,
            },
        },
        MutationPatch::RowDeleted {
            sheet_index,
            row_index,
            count,
        } => types::EditorPatch::RowDeleted {
            patch: types::RowDeletedPatch {
                sheet_index,
                row_index,
                count,
            },
        },
        MutationPatch::ColumnInserted {
            sheet_index,
            col_index,
            count,
        } => types::EditorPatch::ColumnInserted {
            patch: types::ColumnInsertedPatch {
                sheet_index,
                col_index,
                count,
            },
        },
        MutationPatch::ColumnDeleted {
            sheet_index,
            col_index,
            count,
        } => types::EditorPatch::ColumnDeleted {
            patch: types::ColumnDeletedPatch {
                sheet_index,
                col_index,
                count,
            },
        },
        MutationPatch::ImageUpserted { sheet_index, image } => types::EditorPatch::ImageUpserted {
            patch: types::ImageUpsertedPatch {
                sheet_index,
                image: super::status::sheet_image(image),
            },
        },
        MutationPatch::ImageDeleted {
            sheet_index,
            image_id,
        } => types::EditorPatch::ImageDeleted {
            patch: types::ImageDeletedPatch {
                sheet_index,
                image_id,
            },
        },
        MutationPatch::ResyncRequired { reason } => types::EditorPatch::ResyncRequired {
            patch: types::ResyncRequiredPatch { reason },
        },
    }
}
