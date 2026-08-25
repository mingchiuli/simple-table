use crate::application::document_projection;
use crate::document::region_metadata_index::DocumentRegion;
use crate::error::AppError;
use crate::projection_model::{OpenDocumentSnapshot, SheetRegionSnapshot};
use crate::state::{ActiveDocumentRepository, DocumentHandle};

#[cfg(test)]
use crate::application::document_projection::{
    MAX_REGION_ROWS, open_document_snapshot, project_merge_anchor_cells, project_region_cells,
    snapshot_sheet_region, validate_sheet_region,
};
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

pub fn sheet_rows_region_projection_for_command(
    service: &DocumentQueryService,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
    rows: &[usize],
    col_start: usize,
    col_end: usize,
) -> Result<Vec<SheetRegionSnapshot>, AppError> {
    if rows.is_empty() || rows.len() > document_projection::MAX_REGION_ROWS {
        return Err(AppError::ResourceLimitExceeded(format!(
            "rows_region accepts between 1 and {} physical rows",
            document_projection::MAX_REGION_ROWS
        )));
    }
    if rows.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(AppError::DocumentStateInvalid(
            "rows_region physical rows must be sorted and unique".to_string(),
        ));
    }
    let regions = rows
        .iter()
        .map(|row| DocumentRegion {
            sheet_index,
            row_start: *row,
            row_end: row.saturating_add(1),
            col_start,
            col_end,
        })
        .collect::<Vec<_>>();
    for region in &regions {
        document_projection::validate_sheet_region(region)?;
    }
    let handle = document_handle_for_read(service.documents(), document_id)?;
    let editor_state = handle.read_for_command(document_id, base_revision)?;
    regions
        .into_iter()
        .map(|region| document_projection::snapshot_sheet_region(&editor_state, region))
        .collect()
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

fn document_handle_for_read(
    registry: &ActiveDocumentRepository,
    document_id: u64,
) -> Result<std::sync::Arc<DocumentHandle>, AppError> {
    registry.read_handle(document_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::document_format_policy::ensure_native_save_target_allowed;
    use crate::document::region_metadata_index::DocumentRegion;
    use crate::document_data::{DocumentData, DocumentSheet, MergeRange};
    use crate::domain::CellValue;

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
    fn empty_sheet_exposes_one_editable_region_cell() {
        let state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "empty.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    ..Default::default()
                }],
            },
            None,
        );
        let region = DocumentRegion {
            sheet_index: 0,
            row_start: 0,
            row_end: 1,
            col_start: 0,
            col_end: 1,
        };

        let snapshot = document_projection::snapshot_sheet_region(&state, region)
            .expect("an empty sheet keeps one editable cell");

        assert_eq!(snapshot.region, region);
        assert!(snapshot.cells.is_empty());
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
            crate::resource_limits::MAX_SHEET_REGION_RESPONSE_BYTES,
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
            crate::resource_limits::MAX_SHEET_REGION_RESPONSE_BYTES,
        )
        .expect("sized response");
        assert!(response.wire_bytes > 0);
    }

    #[test]
    fn sparse_rows_region_reads_physical_rows_under_one_lock() {
        let state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "rows-region.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    rows: vec![
                        vec![CellValue::String("row-0".to_string())],
                        vec![CellValue::String("row-1".to_string())],
                        vec![CellValue::String("row-2".to_string())],
                    ],
                    ..Default::default()
                }],
            },
            None,
        );
        let document_id = state.document_id();
        let revision = state.revision();
        let registry = ActiveDocumentRepository::default();
        registry.replace_active_for_test(state);
        let service = DocumentQueryService::new(registry.clone());

        let snapshots = sheet_rows_region_projection_for_command(
            &service,
            document_id,
            revision,
            0,
            &[0, 2],
            0,
            1,
        )
        .expect("read sparse rows");

        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].region.row_start, 0);
        assert_eq!(snapshots[1].region.row_start, 2);
        assert_eq!(snapshots[1].cells[0].value.to_display_string(), "row-2");
        assert!(registry.is_write_available_for_test());
        assert!(
            sheet_rows_region_projection_for_command(
                &service,
                document_id,
                revision,
                0,
                &[2, 2],
                0,
                1,
            )
            .is_err()
        );
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

        assert!(error.to_string().contains("export to CSV explicitly"));
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

    #[test]
    fn native_save_keeps_the_source_excel_format() {
        let xlsm = EditorState::with_workbook(
            DocumentData {
                path: "/tmp/macros.xlsm".to_string(),
                file_name: "macros.xlsm".to_string(),
                sheets: vec![DocumentSheet::default()],
            },
            Some(umya_spreadsheet::new_file()),
        );
        let xlsx = EditorState::with_workbook(
            DocumentData {
                path: "/tmp/book.xlsx".to_string(),
                file_name: "book.xlsx".to_string(),
                sheets: vec![DocumentSheet::default()],
            },
            Some(umya_spreadsheet::new_file()),
        );

        assert!(ensure_native_save_target_allowed(&xlsm, "/tmp/macros.xlsm").is_ok());
        assert!(ensure_native_save_target_allowed(&xlsx, "/tmp/book.xlsx").is_ok());
        assert!(ensure_native_save_target_allowed(&xlsm, "/tmp/macros.xlsx").is_err());
        assert!(ensure_native_save_target_allowed(&xlsx, "/tmp/book.xlsm").is_err());

        let csv = EditorState::with_workbook(
            DocumentData {
                path: "/tmp/data.csv".to_string(),
                file_name: "data.csv".to_string(),
                sheets: vec![DocumentSheet::default()],
            },
            None,
        );
        assert!(ensure_native_save_target_allowed(&csv, "/tmp/data.xlsm").is_err());
    }
}
