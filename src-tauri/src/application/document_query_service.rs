use crate::application::{document_format_policy, document_projection};
use crate::document::region_metadata_index::DocumentRegion;
use crate::document_data::DocumentData;
use crate::error::AppError;
use crate::projection_model::{
    DocumentCapabilities, NativeSavePlan, OpenDocumentSnapshot, SheetRegionSnapshot,
    SpreadsheetFormatOptions,
};
#[cfg(any(target_os = "android", target_os = "ios"))]
use crate::state::state::InactiveDocumentPathLease;
use crate::state::state::{ActiveDocumentRepository, DocumentHandle};

#[cfg(test)]
use crate::application::document_format_policy::ensure_native_save_target_allowed;
#[cfg(test)]
use crate::application::document_projection::{
    MAX_REGION_ROWS, open_document_snapshot, project_merge_anchor_cells, project_region_cells,
    snapshot_sheet_region, validate_sheet_region,
};
#[cfg(test)]
use crate::document::capabilities::WorkbookCapabilities;
#[cfg(test)]
use crate::state::editor_state::EditorState;

#[derive(Clone, Default)]
pub struct DocumentQueryService {
    documents: ActiveDocumentRepository,
}

impl DocumentQueryService {
    pub(crate) fn new(documents: ActiveDocumentRepository) -> Self {
        Self { documents }
    }

    fn documents(&self) -> &ActiveDocumentRepository {
        &self.documents
    }
}

/// Restores the frontend after its service state was lost while the Rust process stayed alive.
pub fn active_document_response(
    service: &DocumentQueryService,
) -> Result<Option<OpenDocumentSnapshot>, AppError> {
    let handle = service.documents().active_handle()?;
    handle
        .map(|handle| {
            let editor_state = handle.read()?;
            Ok(document_projection::open_document_snapshot(&editor_state))
        })
        .transpose()
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub(crate) fn begin_inactive_document_path_operation(
    service: &DocumentQueryService,
    path: &str,
) -> Result<Option<InactiveDocumentPathLease>, AppError> {
    service.documents().begin_inactive_path_operation(path)
}

pub fn current_document_projection_for_command(
    service: &DocumentQueryService,
    document_id: u64,
    base_revision: u64,
    preferred_sheet_index: usize,
) -> Result<OpenDocumentSnapshot, AppError> {
    let handle = document_handle_for_read(service.documents(), document_id)?;
    let response = {
        let editor_state = handle.read_for_command(document_id, base_revision)?;
        document_projection::open_document_snapshot_for_sheet(&editor_state, preferred_sheet_index)
    };
    Ok(response)
}

pub fn sheet_region_projection_for_command(
    service: &DocumentQueryService,
    document_id: u64,
    base_revision: u64,
    region: DocumentRegion,
) -> Result<SheetRegionSnapshot, AppError> {
    document_projection::validate_sheet_region(&region)?;
    sheet_region_snapshot_for_command(service, document_id, base_revision, region)
}

fn sheet_region_snapshot_for_command(
    service: &DocumentQueryService,
    document_id: u64,
    base_revision: u64,
    region: DocumentRegion,
) -> Result<SheetRegionSnapshot, AppError> {
    sheet_region_snapshot_from_registry(service.documents(), document_id, base_revision, region)
}

fn sheet_region_snapshot_from_registry(
    registry: &ActiveDocumentRepository,
    document_id: u64,
    base_revision: u64,
    region: DocumentRegion,
) -> Result<SheetRegionSnapshot, AppError> {
    let handle = document_handle_for_read(registry, document_id)?;
    let editor_state = handle.read_for_command(document_id, base_revision)?;
    document_projection::snapshot_sheet_region(&editor_state, region)
}

pub(crate) fn inspect_active_file_if_command_matches<T>(
    service: &DocumentQueryService,
    document_id: u64,
    base_revision: u64,
    inspect: impl FnOnce(&DocumentData) -> T,
) -> Result<Option<T>, AppError> {
    let Some(handle) = service.documents().active_handle()? else {
        return Ok(None);
    };
    if handle.document_id() != document_id {
        return Ok(None);
    }
    let editor_state = match handle.read() {
        Ok(editor_state) => editor_state,
        Err(AppError::DocumentStateInvalid(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    if editor_state.revision() != base_revision {
        return Ok(None);
    }
    Ok(Some(inspect(editor_state.file_data())))
}

#[cfg(test)]
pub fn document_capabilities(
    service: &DocumentQueryService,
    file_name: &str,
    current_path: Option<&str>,
) -> DocumentCapabilities {
    document_format_policy::capabilities_for_source(
        file_name,
        current_path,
        active_workbook_capabilities(service, file_name, current_path),
    )
}

pub fn document_capabilities_for_command(
    service: &DocumentQueryService,
    document_id: u64,
    base_revision: u64,
) -> Result<DocumentCapabilities, AppError> {
    let handle = document_handle_for_read(service.documents(), document_id)?;
    let editor_state = handle.read_for_command(document_id, base_revision)?;
    Ok(document_format_policy::document_capabilities(&editor_state))
}

pub fn native_save_plan_for_command(
    service: &DocumentQueryService,
    document_id: u64,
    base_revision: u64,
    target_path_or_name: &str,
) -> Result<NativeSavePlan, AppError> {
    let handle = document_handle_for_read(service.documents(), document_id)?;
    let editor_state = handle.read_for_command(document_id, base_revision)?;
    Ok(document_format_policy::native_save_plan(
        &editor_state,
        target_path_or_name,
    ))
}

pub fn format_options() -> SpreadsheetFormatOptions {
    document_format_policy::format_options()
}

pub fn sheet_images_for_command(
    service: &DocumentQueryService,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    offset: usize,
    limit: usize,
) -> Result<(Vec<crate::document_data::SheetImage>, Option<usize>), AppError> {
    const MAX_PAGE_SIZE: usize = 256;
    let handle = document_handle_for_read(service.documents(), document_id)?;
    let editor_state = handle.read_for_command(document_id, base_revision)?;
    let sheet = editor_state
        .file_data()
        .sheets
        .get(sheet_index)
        .ok_or(AppError::InvalidSheetIndex(sheet_index))?;
    let limit = limit.clamp(1, MAX_PAGE_SIZE);
    let end = offset.saturating_add(limit).min(sheet.rich.images.len());
    let items = sheet
        .rich
        .images
        .get(offset..end)
        .unwrap_or_default()
        .to_vec();
    let next_offset = (end < sheet.rich.images.len()).then_some(end);
    Ok((items, next_offset))
}

pub fn image_bytes_for_command(
    service: &DocumentQueryService,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    image_id: &str,
) -> Result<std::sync::Arc<[u8]>, AppError> {
    let handle = document_handle_for_read(service.documents(), document_id)?;
    let editor_state = handle.read_for_command(document_id, base_revision)?;
    editor_state
        .image_bytes(sheet_index, image_id)
        .ok_or_else(|| AppError::DocumentStateInvalid(format!("image {image_id} is unavailable")))
}

#[cfg(test)]
fn active_workbook_capabilities(
    service: &DocumentQueryService,
    file_name: &str,
    current_path: Option<&str>,
) -> WorkbookCapabilities {
    let Ok(handle) = service.documents().active_handle() else {
        eprintln!("document registry unavailable while reading workbook capabilities");
        return WorkbookCapabilities::default();
    };
    if let Some(handle) = handle
        && let Ok(editor_state) = handle.read()
    {
        let active_file = editor_state.file_data();
        let matches = match current_path {
            Some(path) if !path.is_empty() => path == active_file.path,
            _ => active_file.file_name == file_name,
        };
        if matches {
            return editor_state.capabilities();
        }
    }
    WorkbookCapabilities::default()
}

fn document_handle_for_read(
    registry: &ActiveDocumentRepository,
    document_id: u64,
) -> Result<std::sync::Arc<DocumentHandle>, AppError> {
    registry.read_handle(document_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::region_metadata_index::DocumentRegion;
    use crate::document_data::{DocumentSheet, MergeRange};
    use crate::domain::CellValue;

    #[test]
    fn document_capabilities_are_computed_by_backend() {
        let service = DocumentQueryService::default();
        assert_eq!(
            document_capabilities(&service, "book.xlsx", None),
            DocumentCapabilities {
                source_format: "xlsx".to_string(),
                can_save_original: true,
                native_save_format: Some("xlsx".to_string()),
                export_formats: vec!["xlsx".to_string(), "csv".to_string()],
                native_save_extension: Some("xlsx".to_string()),
                export_extension: "xlsx".to_string(),
                requires_save_as_for_native_save: false,
                workbook: WorkbookCapabilities::default(),
            }
        );
        assert_eq!(
            document_capabilities(&service, "data.csv", Some("/tmp/data.csv")),
            DocumentCapabilities {
                source_format: "csv".to_string(),
                can_save_original: true,
                native_save_format: Some("csv".to_string()),
                export_formats: vec!["xlsx".to_string(), "csv".to_string()],
                native_save_extension: Some("csv".to_string()),
                export_extension: "csv".to_string(),
                requires_save_as_for_native_save: false,
                workbook: WorkbookCapabilities::default(),
            }
        );
    }

    #[test]
    fn optional_active_file_inspection_treats_replaced_context_as_unavailable() {
        let documents = ActiveDocumentRepository::default();
        let service = DocumentQueryService::new(documents.clone());
        let first_id = documents.replace_active_for_test(EditorState::with_workbook(
            DocumentData {
                path: "/tmp/first.xlsx".to_string(),
                file_name: "first.xlsx".to_string(),
                sheets: vec![DocumentSheet::default()],
            },
            None,
        ));

        assert_eq!(
            inspect_active_file_if_command_matches(&service, first_id, 0, |file| {
                file.path.clone()
            })
            .expect("inspect current file"),
            Some("/tmp/first.xlsx".to_string())
        );

        documents.replace_active_for_test(EditorState::with_workbook(
            DocumentData {
                path: "/tmp/second.xlsx".to_string(),
                file_name: "second.xlsx".to_string(),
                sheets: vec![DocumentSheet::default()],
            },
            None,
        ));

        assert!(matches!(
            inspect_active_file_if_command_matches(&service, first_id, 0, |_| ()),
            Ok(None)
        ));
    }

    #[test]
    fn open_document_response_contains_manifest_and_initial_region() {
        let first_sheet = DocumentSheet {
            name: "First".to_string(),
            rows: vec![vec![CellValue::String("loaded".to_string())]],
            ..Default::default()
        };
        let second_sheet = DocumentSheet {
            name: "Second".to_string(),
            rows: vec![vec![CellValue::String("deferred".to_string())]],
            ..Default::default()
        };
        let state = EditorState::with_workbook(
            DocumentData {
                path: "/tmp/book.xlsx".to_string(),
                file_name: "book.xlsx".to_string(),
                sheets: vec![first_sheet, second_sheet],
            },
            None,
        );

        let response = open_document_snapshot(&state);

        assert_eq!(response.document.sheets[0].name, "First");
        assert_eq!(response.document.sheets[1].name, "Second");
        assert_eq!(
            response
                .document
                .sheets
                .iter()
                .map(|sheet| sheet.extent)
                .collect::<Vec<_>>(),
            vec![
                crate::document_data::SheetExtent {
                    row_count: 1,
                    column_count: 1,
                },
                crate::document_data::SheetExtent {
                    row_count: 1,
                    column_count: 1,
                },
            ]
        );
        let initial = response.initial_region.expect("initial region");
        assert_eq!(initial.region.sheet_index, 0);
        assert_eq!(initial.cells.len(), 1);
    }

    #[test]
    fn region_projection_keeps_absolute_cell_coordinates() {
        let sheet = DocumentSheet {
            rows: vec![
                vec![
                    CellValue::String("A1".into()),
                    CellValue::String("B1".into()),
                ],
                vec![
                    CellValue::String("A2".into()),
                    CellValue::String("B2".into()),
                ],
            ],
            ..Default::default()
        };
        let region = DocumentRegion {
            sheet_index: 0,
            row_start: 1,
            row_end: 2,
            col_start: 1,
            col_end: 2,
        };

        let cells = project_region_cells(&sheet, &region);

        assert_eq!(cells.len(), 1);
        assert_eq!((cells[0].row, cells[0].col), (1, 1));
        assert_eq!(cells[0].display.as_deref(), Some("B2"));
    }

    #[test]
    fn region_projection_reports_and_enforces_final_serialized_size() {
        let state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "region.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    rows: vec![vec![CellValue::String("value".to_string())]],
                    ..Default::default()
                }],
            },
            None,
        );
        let snapshot = snapshot_sheet_region(
            &state,
            DocumentRegion {
                sheet_index: 0,
                row_start: 0,
                row_end: 1,
                col_start: 0,
                col_end: 1,
            },
        )
        .expect("region snapshot");
        let response = crate::protocol_projection::sheet_region_response(
            snapshot,
            crate::editor_protocol::MAX_SHEET_REGION_RESPONSE_BYTES,
        )
        .expect("region response");
        let serialized_bytes = serde_json::to_vec(&response)
            .expect("serialize response")
            .len();

        assert_eq!(response.wire_bytes, serialized_bytes);
        let unbounded = snapshot_sheet_region(
            &state,
            DocumentRegion {
                sheet_index: 0,
                row_start: 0,
                row_end: 1,
                col_start: 0,
                col_end: 1,
            },
        )
        .expect("region snapshot");
        assert!(matches!(
            crate::protocol_projection::sheet_region_response(unbounded, 1),
            Err(AppError::RegionResponseTooLarge {
                maximum_bytes: 1,
                ..
            })
        ));
    }

    #[test]
    fn region_snapshot_releases_document_lock_before_response_sizing() {
        let state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "region.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    rows: vec![vec![CellValue::String("value".to_string())]],
                    ..Default::default()
                }],
            },
            None,
        );
        let document_id = state.document_id();
        let revision = state.revision();
        let registry = ActiveDocumentRepository::default();
        registry.replace_active_for_test(state);

        let snapshot = sheet_region_snapshot_from_registry(
            &registry,
            document_id,
            revision,
            DocumentRegion {
                sheet_index: 0,
                row_start: 0,
                row_end: 1,
                col_start: 0,
                col_end: 1,
            },
        )
        .expect("region snapshot");
        assert!(registry.is_write_available_for_test());

        let response = crate::protocol_projection::sheet_region_response(
            snapshot,
            crate::editor_protocol::MAX_SHEET_REGION_RESPONSE_BYTES,
        )
        .expect("sized response");
        assert!(response.wire_bytes > 0);
    }

    #[test]
    fn region_projection_includes_merge_anchor_outside_region() {
        let sheet = DocumentSheet {
            rows: vec![vec![CellValue::String("anchor".to_string())]],
            merges: vec![MergeRange {
                start_row: 0,
                start_col: 0,
                end_row: 140,
                end_col: 0,
            }],
            ..Default::default()
        };
        let region = DocumentRegion {
            sheet_index: 0,
            row_start: 128,
            row_end: 141,
            col_start: 0,
            col_end: 1,
        };
        let file_data = DocumentData {
            path: String::new(),
            file_name: "merge.xlsx".to_string(),
            sheets: vec![sheet.clone()],
        };
        let metadata =
            crate::document::region_metadata_index::RegionMetadataIndex::from_file_data(&file_data)
                .project(&file_data, &region);
        let anchors = project_merge_anchor_cells(&sheet, &region, &metadata.merges);

        assert_eq!(anchors.len(), 1);
        assert_eq!(anchors[0].row, 0);
        assert_eq!(anchors[0].col, 0);
        assert_eq!(anchors[0].value, CellValue::String("anchor".to_string()));
    }

    #[test]
    fn region_projection_rejects_degenerate_oversized_dimensions() {
        let region = DocumentRegion {
            sheet_index: 0,
            row_start: 0,
            row_end: MAX_REGION_ROWS + 1,
            col_start: 0,
            col_end: 0,
        };

        assert!(matches!(
            validate_sheet_region(&region),
            Err(AppError::ResourceLimitExceeded(_))
        ));
    }

    #[test]
    fn native_save_rejects_lossy_csv_conversion_but_export_remains_available() {
        let state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "untitled.xlsx".to_string(),
                sheets: vec![DocumentSheet::default(), DocumentSheet::default()],
            },
            None,
        );

        let error = ensure_native_save_target_allowed(&state, "converted.csv")
            .expect_err("native save must reject lossy CSV conversion");

        assert!(error.to_string().contains("use Export instead"));
        assert!(state.generate_file_bytes_for_target("export.csv").is_ok());
    }

    #[test]
    fn native_save_allows_an_existing_csv_document_to_remain_csv() {
        let state = EditorState::with_workbook(
            DocumentData {
                path: "/tmp/data.csv".to_string(),
                file_name: "data.csv".to_string(),
                sheets: vec![DocumentSheet::default()],
            },
            None,
        );

        assert!(ensure_native_save_target_allowed(&state, "/tmp/data.csv").is_ok());
    }
}
