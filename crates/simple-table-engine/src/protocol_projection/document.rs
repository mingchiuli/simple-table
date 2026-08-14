use crate::error::AppError;
use crate::projection_model::{
    DocumentManifestSnapshot, OpenDocumentSnapshot, SavedDocumentOutcome, SheetRegionSnapshot,
};
use crate::resource_limits::{MAX_DOCUMENT_RESPONSE_BYTES, MAX_SHEET_REGION_RESPONSE_BYTES};
use crate::types;

use super::cell::{projected_cell_change, region_metadata, sheet_manifest, sheet_region};
use super::size::serialized_json_bytes;
use super::status::editor_session;

pub(crate) fn open_document_response(
    value: OpenDocumentSnapshot,
) -> Result<types::OpenDocumentResponse, AppError> {
    bound_open_document_response(
        project_open_document_response(value),
        MAX_DOCUMENT_RESPONSE_BYTES,
    )
}

fn project_open_document_response(value: OpenDocumentSnapshot) -> types::OpenDocumentResponse {
    types::OpenDocumentResponse {
        document: document_manifest(value.document),
        editor_session: editor_session(value.editor_session),
        initial_region: value
            .initial_region
            .and_then(|region| sheet_region_response(region, MAX_SHEET_REGION_RESPONSE_BYTES).ok()),
    }
}

fn bound_open_document_response(
    mut response: types::OpenDocumentResponse,
    maximum_bytes: usize,
) -> Result<types::OpenDocumentResponse, AppError> {
    if serialized_json_bytes(&response)? <= maximum_bytes {
        return Ok(response);
    }
    response.initial_region = None;
    ensure_document_response_is_bounded(&response, maximum_bytes)?;
    Ok(response)
}

pub(crate) fn saved_document_response(
    value: SavedDocumentOutcome,
) -> Result<types::SavedDocumentResponse, AppError> {
    let response = types::SavedDocumentResponse {
        document: value.document.map(document_manifest),
        identity: value.identity.map(|identity| types::SavedDocumentIdentity {
            path: identity.path,
            file_name: identity.file_name,
        }),
        editor_session: editor_session(value.editor_session),
    };
    ensure_document_response_is_bounded(&response, MAX_DOCUMENT_RESPONSE_BYTES)?;
    Ok(response)
}

fn ensure_document_response_is_bounded(
    response: &impl serde::Serialize,
    maximum_bytes: usize,
) -> Result<(), AppError> {
    let bytes = serialized_json_bytes(response)?;
    if bytes > maximum_bytes {
        return Err(AppError::ResourceLimitExceeded(format!(
            "document response is {bytes} bytes, maximum is {maximum_bytes} bytes"
        )));
    }
    Ok(())
}

pub(crate) fn sheet_region_response(
    value: SheetRegionSnapshot,
    maximum_bytes: usize,
) -> Result<types::SheetRegionProjectionResponse, AppError> {
    let mut response = types::SheetRegionProjectionResponse {
        document_id: value.document_id,
        revision: value.revision,
        region: sheet_region(value.region),
        cells: value.cells.into_iter().map(projected_cell_change).collect(),
        merge_anchor_cells: value
            .merge_anchor_cells
            .into_iter()
            .map(projected_cell_change)
            .collect(),
        metadata: region_metadata(value.metadata),
        wire_bytes: 0,
    };
    let mut estimate = serialized_json_bytes(&response)?;
    for _ in 0..8 {
        response.wire_bytes = estimate;
        let actual = serialized_json_bytes(&response)?;
        if actual == estimate {
            if actual > maximum_bytes {
                return Err(AppError::RegionResponseTooLarge {
                    wire_bytes: actual,
                    maximum_bytes,
                });
            }
            return Ok(response);
        }
        estimate = actual;
    }
    Err(AppError::Internal(
        "failed to converge while sizing region response".to_string(),
    ))
}

fn document_manifest(value: DocumentManifestSnapshot) -> types::DocumentManifest {
    types::DocumentManifest {
        path: value.path,
        file_name: value.file_name,
        sheets: value.sheets.into_iter().map(sheet_manifest).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::document_projection;
    use crate::document_data::{DocumentData, DocumentSheet};
    use crate::state::editor_state::EditorState;

    #[test]
    fn open_response_drops_the_optional_region_before_exceeding_its_total_budget() {
        let state = EditorState::new(DocumentData {
            path: "/tmp/book.xlsx".to_string(),
            file_name: "book.xlsx".to_string(),
            sheets: vec![DocumentSheet {
                rows: vec![vec![crate::domain::CellValue::String("value".to_string())]],
                ..Default::default()
            }],
        });
        let response =
            project_open_document_response(document_projection::open_document_snapshot(&state));
        assert!(response.initial_region.is_some());
        let mut manifest_only = response.clone();
        manifest_only.initial_region = None;
        let manifest_bytes = serialized_json_bytes(&manifest_only).unwrap();
        assert!(serialized_json_bytes(&response).unwrap() > manifest_bytes);

        let bounded = bound_open_document_response(response, manifest_bytes)
            .expect("manifest-only response fits");

        assert!(bounded.initial_region.is_none());
        assert!(serialized_json_bytes(&bounded).unwrap() <= manifest_bytes);
        assert!(matches!(
            bound_open_document_response(manifest_only, manifest_bytes - 1),
            Err(AppError::ResourceLimitExceeded(_))
        ));
    }
}
