use crate::error::AppError;
use crate::io::file_format::{
    default_spreadsheet_extension, export_extensions, extension_of, spreadsheet_format_options,
    supported_extension_from_name,
};
use crate::ops::patch_projector::editor_state_info;
use crate::state::{
    active_document_store,
    editor_state::EditorState,
    state::{ActiveDocumentStore, DocumentHandle},
};
use crate::types::{
    DocumentCapabilities, DocumentManifest, EditorSessionInfo, FileData, NativeSavePlan,
    OpenDocumentResponse, SheetData, SheetLayoutProjection, SheetManifest, SheetRegion,
    SheetRegionProjectionResponse, SpreadsheetFormatOptions, WorkbookCapabilities,
};
use std::io::Write;

const LOSSY_CSV_SAVE_REASON: &str = "Saving a non-CSV document as CSV would discard sheets, formulas, or formatting; use Export instead.";
const INITIAL_REGION_ROWS: usize = 128;
const INITIAL_REGION_COLUMNS: usize = 32;
const MAX_REGION_CELLS: usize = 65_536;
const MAX_REGION_ROWS: usize = 1_024;
const MAX_REGION_COLUMNS: usize = 512;
const MAX_REGION_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

/// Restores the frontend after its runtime state was lost while the Rust process stayed alive.
pub fn active_document_response() -> Result<Option<OpenDocumentResponse>, AppError> {
    let registry = active_document_store();
    let handle = {
        let registry_guard = registry
            .read()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        registry_guard.active_handle()
    };
    handle
        .map(|handle| {
            let editor_state = handle.read()?;
            Ok(finalize_open_document_response(
                open_document_response_snapshot(&editor_state),
            ))
        })
        .transpose()
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub(crate) fn active_document_path() -> Result<Option<String>, AppError> {
    let registry = active_document_store();
    let handle = registry
        .read()
        .map_err(|_| AppError::poisoned_lock("document registry"))?
        .active_handle();
    Ok(handle
        .map(|handle| {
            handle
                .read()
                .map(|editor_state| editor_state.file_data().path.clone())
        })
        .transpose()?
        .filter(|path| !path.is_empty()))
}

pub fn current_document_projection_for_command(
    document_id: u64,
    base_revision: u64,
    preferred_sheet_index: usize,
) -> Result<OpenDocumentResponse, AppError> {
    let registry = active_document_store();
    let handle = document_handle_for_read(&registry, document_id)?;
    let response = {
        let editor_state = handle.read_for_command(document_id, base_revision)?;
        open_document_response_snapshot_for_sheet(&editor_state, preferred_sheet_index)
    };
    Ok(finalize_open_document_response(response))
}

pub fn sheet_region_projection_for_command(
    document_id: u64,
    base_revision: u64,
    region: SheetRegion,
) -> Result<SheetRegionProjectionResponse, AppError> {
    validate_sheet_region(&region)?;
    let response = sheet_region_snapshot_for_command(document_id, base_revision, region)?;
    finalize_region_response(response, MAX_REGION_RESPONSE_BYTES)
}

fn sheet_region_snapshot_for_command(
    document_id: u64,
    base_revision: u64,
    region: SheetRegion,
) -> Result<SheetRegionProjectionResponse, AppError> {
    let registry = active_document_store();
    sheet_region_snapshot_from_registry(&registry, document_id, base_revision, region)
}

fn sheet_region_snapshot_from_registry(
    registry: &std::sync::Arc<std::sync::RwLock<crate::state::state::ActiveDocumentStore>>,
    document_id: u64,
    base_revision: u64,
    region: SheetRegion,
) -> Result<SheetRegionProjectionResponse, AppError> {
    let handle = document_handle_for_read(registry, document_id)?;
    let editor_state = handle.read_for_command(document_id, base_revision)?;
    snapshot_sheet_region(&editor_state, region)
}

pub(crate) fn inspect_current_file_for_command<T>(
    document_id: u64,
    base_revision: u64,
    inspect: impl FnOnce(&FileData) -> T,
) -> Result<T, AppError> {
    let registry = active_document_store();
    let handle = document_handle_for_read(&registry, document_id)?;
    let editor_state = handle.read_for_command(document_id, base_revision)?;
    Ok(inspect(editor_state.file_data()))
}

#[cfg(test)]
pub fn document_capabilities(file_name: &str, current_path: Option<&str>) -> DocumentCapabilities {
    let source_name = current_path.unwrap_or(file_name);
    let source_format = document_format(source_name)
        .or_else(|| document_format(file_name))
        .unwrap_or_else(default_extension_string);
    let native_extension = native_save_extension(source_name);
    let native_save_allowed = native_extension.is_some();
    let export_extension = export_extension(file_name).unwrap_or_else(|| source_format.clone());
    let export_formats = export_formats_for(&source_format);

    DocumentCapabilities {
        source_format,
        can_save_original: native_save_allowed,
        native_save_format: native_extension.clone(),
        export_formats,
        requires_save_as_for_native_save: native_extension.is_none(),
        native_save_extension: native_extension,
        export_extension,
        workbook: active_workbook_capabilities(file_name, current_path, native_save_allowed),
    }
}

pub fn document_capabilities_for_command(
    document_id: u64,
    base_revision: u64,
) -> Result<DocumentCapabilities, AppError> {
    let (file_name, current_path) =
        inspect_current_file_for_command(document_id, base_revision, |file_data| {
            (
                file_data.file_name.clone(),
                (!file_data.path.is_empty()).then(|| file_data.path.clone()),
            )
        })?;
    let current_path = current_path.as_deref();
    let file_name = file_name.as_str();
    let source_name = current_path.unwrap_or(file_name);
    let source_format = document_format(source_name)
        .or_else(|| document_format(file_name))
        .unwrap_or_else(default_extension_string);
    let native_extension = native_save_extension(source_name);
    let native_save_allowed = native_extension.is_some();
    let export_extension = export_extension(file_name).unwrap_or_else(|| source_format.clone());
    let export_formats = export_formats_for(&source_format);
    let workbook =
        workbook_capabilities_for_command(document_id, base_revision, native_save_allowed)?;

    Ok(DocumentCapabilities {
        source_format,
        can_save_original: native_save_allowed,
        native_save_format: native_extension.clone(),
        export_formats,
        requires_save_as_for_native_save: native_extension.is_none(),
        native_save_extension: native_extension,
        export_extension,
        workbook,
    })
}

pub fn native_save_plan_for_command(
    document_id: u64,
    base_revision: u64,
    target_path_or_name: &str,
) -> Result<NativeSavePlan, AppError> {
    let source_format =
        document_format(target_path_or_name).unwrap_or_else(default_extension_string);
    let native_extension = native_save_extension(target_path_or_name);
    let native_save_allowed = native_extension.is_some();
    let export_extension =
        export_extension(target_path_or_name).unwrap_or_else(|| source_format.clone());
    let export_formats = export_formats_for(&source_format);
    let workbook = native_save_workbook_capabilities_for_command(
        document_id,
        base_revision,
        native_save_allowed,
        target_path_or_name,
    )?;
    let capabilities = DocumentCapabilities {
        source_format,
        can_save_original: native_save_allowed,
        native_save_format: native_extension.clone(),
        export_formats,
        requires_save_as_for_native_save: native_extension.is_none(),
        native_save_extension: native_extension.clone(),
        export_extension,
        workbook,
    };
    let blocked_reason = native_save_blocked_reason(&capabilities);

    Ok(NativeSavePlan {
        can_save: blocked_reason.is_none(),
        requires_save_as: capabilities.requires_save_as_for_native_save,
        native_save_extension: native_extension.clone(),
        default_extension: native_extension.unwrap_or_else(default_extension_string),
        blocked_reason,
        capabilities,
    })
}

pub fn format_options() -> SpreadsheetFormatOptions {
    spreadsheet_format_options()
}

#[cfg(test)]
fn active_workbook_capabilities(
    file_name: &str,
    current_path: Option<&str>,
    native_save_allowed: bool,
) -> WorkbookCapabilities {
    let registry = active_document_store();
    let Ok(registry_guard) = registry.read() else {
        eprintln!("document registry lock poisoned while reading workbook capabilities");
        let mut capabilities = WorkbookCapabilities::default();
        capabilities.save.can_native_save = native_save_allowed;
        return capabilities;
    };
    let handle = registry_guard.active_handle();
    drop(registry_guard);
    if let Some(handle) = handle
        && let Ok(editor_state) = handle.read()
    {
        let active_file = editor_state.file_data();
        let matches = match current_path {
            Some(path) if !path.is_empty() => path == active_file.path,
            _ => active_file.file_name == file_name,
        };
        if matches {
            let mut capabilities = editor_state.capabilities();
            capabilities.save.can_native_save =
                native_save_allowed && capabilities.save.can_native_save;
            return capabilities;
        }
    }
    let mut capabilities = WorkbookCapabilities::default();
    capabilities.save.can_native_save = native_save_allowed;
    capabilities
}

fn workbook_capabilities_for_command(
    document_id: u64,
    base_revision: u64,
    native_save_allowed: bool,
) -> Result<WorkbookCapabilities, AppError> {
    workbook_capabilities_for_command_and_target(
        document_id,
        base_revision,
        native_save_allowed,
        None,
    )
}

fn native_save_workbook_capabilities_for_command(
    document_id: u64,
    base_revision: u64,
    native_save_allowed: bool,
    target_path_or_name: &str,
) -> Result<WorkbookCapabilities, AppError> {
    workbook_capabilities_for_command_and_target(
        document_id,
        base_revision,
        native_save_allowed,
        Some(target_path_or_name),
    )
}

fn workbook_capabilities_for_command_and_target(
    document_id: u64,
    base_revision: u64,
    native_save_allowed: bool,
    target_path_or_name: Option<&str>,
) -> Result<WorkbookCapabilities, AppError> {
    let registry = active_document_store();
    let handle = document_handle_for_read(&registry, document_id)?;
    let editor_state = handle.read_for_command(document_id, base_revision)?;
    let mut capabilities = editor_state.capabilities();
    capabilities.save.can_native_save = native_save_allowed && capabilities.save.can_native_save;
    if let Some(reason) = target_path_or_name
        .and_then(|target| native_save_target_block_reason(&editor_state, target))
    {
        capabilities.save.can_native_save = false;
        if !capabilities
            .save
            .blocked_save_reasons
            .iter()
            .any(|item| item == reason)
        {
            capabilities
                .save
                .blocked_save_reasons
                .push(reason.to_string());
        }
    }
    Ok(capabilities)
}

fn document_handle_for_read(
    registry: &std::sync::Arc<std::sync::RwLock<ActiveDocumentStore>>,
    document_id: u64,
) -> Result<std::sync::Arc<DocumentHandle>, AppError> {
    registry
        .read()
        .map_err(|_| AppError::poisoned_lock("document registry"))?
        .active_handle_for_read(document_id)
}

pub(crate) fn ensure_native_save_target_allowed(
    editor_state: &EditorState,
    target_path_or_name: &str,
) -> Result<(), AppError> {
    if let Some(reason) = native_save_target_block_reason(editor_state, target_path_or_name) {
        return Err(AppError::DocumentStateInvalid(reason.to_string()));
    }
    Ok(())
}

fn native_save_target_block_reason(
    editor_state: &EditorState,
    target_path_or_name: &str,
) -> Option<&'static str> {
    let target_extension =
        extension_of(target_path_or_name).unwrap_or_else(default_extension_string);
    (target_extension == "csv" && !editor_state.is_csv_backed()).then_some(LOSSY_CSV_SAVE_REASON)
}

fn native_save_blocked_reason(capabilities: &DocumentCapabilities) -> Option<String> {
    if capabilities.native_save_extension.is_none() {
        return Some("Native save is only supported as .xlsx or .csv.".to_string());
    }
    if !capabilities.workbook.save.can_native_save {
        return Some(first_reason(
            [
                &capabilities.workbook.save.blocked_save_reasons,
                &capabilities.workbook.structure.blocked_structure_reasons,
                &capabilities
                    .workbook
                    .structure
                    .blocked_sheet_structure_reasons,
                &capabilities.workbook.save.detected_features,
            ],
            "Workbook cannot be saved in its current state.",
        ));
    }
    None
}

fn first_reason<const N: usize>(reason_groups: [&Vec<String>; N], fallback: &str) -> String {
    reason_groups
        .into_iter()
        .flat_map(|reasons| reasons.iter())
        .next()
        .cloned()
        .unwrap_or_else(|| fallback.to_string())
}

pub(crate) fn editor_session_info(editor_state: &EditorState) -> EditorSessionInfo {
    EditorSessionInfo {
        document_id: editor_state.document_id(),
        revision: editor_state.revision(),
        formula_status: editor_state.formula_status().bounded(100),
        capabilities: editor_state.capabilities(),
        editor_state: editor_state_info(editor_state),
    }
}

pub(crate) fn open_document_response_snapshot(editor_state: &EditorState) -> OpenDocumentResponse {
    open_document_response_snapshot_for_sheet(editor_state, 0)
}

fn open_document_response_snapshot_for_sheet(
    editor_state: &EditorState,
    preferred_sheet_index: usize,
) -> OpenDocumentResponse {
    let initial_region = editor_state
        .sheet_extent(preferred_sheet_index)
        .map(|extent| initial_sheet_region(preferred_sheet_index, &extent))
        .and_then(|region| snapshot_sheet_region(editor_state, region).ok());
    OpenDocumentResponse {
        document: document_manifest(editor_state),
        editor_session: editor_session_info(editor_state),
        initial_region,
    }
}

pub(crate) fn document_manifest(editor_state: &EditorState) -> DocumentManifest {
    let source = editor_state.file_data();
    let extents = editor_state.sheet_extents();
    DocumentManifest {
        path: source.path.clone(),
        file_name: source.file_name.clone(),
        sheets: source
            .sheets
            .iter()
            .zip(extents)
            .map(|(sheet, extent)| SheetManifest {
                name: sheet.name.clone(),
                extent,
                layout: sheet_layout_projection(sheet),
            })
            .collect(),
    }
}

fn snapshot_sheet_region(
    editor_state: &EditorState,
    region: SheetRegion,
) -> Result<SheetRegionProjectionResponse, AppError> {
    let sheet = editor_state
        .file_data()
        .sheets
        .get(region.sheet_index)
        .ok_or(AppError::InvalidSheetIndex(region.sheet_index))?;
    let extent = editor_state
        .sheet_extent(region.sheet_index)
        .ok_or(AppError::InvalidSheetIndex(region.sheet_index))?;
    if region.row_end > extent.row_count || region.col_end > extent.column_count {
        return Err(AppError::DocumentStateInvalid(
            "sheet region exceeds the current sheet extent".to_string(),
        ));
    }
    let metadata = editor_state.region_metadata(&region);
    let cells = project_region_cells(sheet, &region);
    let merge_anchor_cells = project_merge_anchor_cells(sheet, &region, &metadata.merges);
    Ok(SheetRegionProjectionResponse {
        document_id: editor_state.document_id(),
        revision: editor_state.revision(),
        region,
        cells,
        merge_anchor_cells,
        metadata,
        estimated_bytes: None,
    })
}

pub(crate) fn finalize_open_document_response(
    mut response: OpenDocumentResponse,
) -> OpenDocumentResponse {
    response.initial_region = response
        .initial_region
        .and_then(|region| finalize_region_response(region, MAX_REGION_RESPONSE_BYTES).ok());
    response
}

fn finalize_region_response(
    mut response: SheetRegionProjectionResponse,
    maximum_bytes: usize,
) -> Result<SheetRegionProjectionResponse, AppError> {
    response.estimated_bytes = None;
    let mut estimate = serialized_json_bytes(&response)?;
    for _ in 0..8 {
        response.estimated_bytes = Some(estimate);
        let actual = serialized_json_bytes(&response)?;
        if actual == estimate {
            if actual > maximum_bytes {
                return Err(AppError::RegionResponseTooLarge {
                    estimated_bytes: actual,
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

fn serialized_json_bytes(value: &impl serde::Serialize) -> Result<usize, AppError> {
    let mut counter = CountingWriter::default();
    serde_json::to_writer(&mut counter, value)
        .map_err(|error| AppError::Internal(format!("failed to size region response: {error}")))?;
    Ok(counter.bytes)
}

#[derive(Default)]
struct CountingWriter {
    bytes: usize,
}

impl Write for CountingWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.bytes = self.bytes.saturating_add(buffer.len());
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn initial_sheet_region(sheet_index: usize, extent: &crate::types::SheetExtent) -> SheetRegion {
    SheetRegion {
        sheet_index,
        row_start: 0,
        row_end: extent.row_count.min(INITIAL_REGION_ROWS),
        col_start: 0,
        col_end: extent.column_count.min(INITIAL_REGION_COLUMNS),
    }
}

fn validate_sheet_region(region: &SheetRegion) -> Result<(), AppError> {
    if region.row_start > region.row_end || region.col_start > region.col_end {
        return Err(AppError::DocumentStateInvalid(
            "invalid sheet region bounds".to_string(),
        ));
    }
    let cells = region
        .row_end
        .saturating_sub(region.row_start)
        .saturating_mul(region.col_end.saturating_sub(region.col_start));
    let row_count = region.row_end.saturating_sub(region.row_start);
    let column_count = region.col_end.saturating_sub(region.col_start);
    if row_count > MAX_REGION_ROWS || column_count > MAX_REGION_COLUMNS {
        return Err(AppError::ResourceLimitExceeded(format!(
            "sheet region dimensions are {row_count}x{column_count}, maximum is {MAX_REGION_ROWS}x{MAX_REGION_COLUMNS}"
        )));
    }
    if cells > MAX_REGION_CELLS {
        return Err(AppError::ResourceLimitExceeded(format!(
            "sheet region contains {cells} cells, maximum is {MAX_REGION_CELLS}"
        )));
    }
    if region.row_end > crate::domain::resource_limits::MAX_ROWS_PER_SHEET
        || region.col_end > crate::domain::resource_limits::MAX_COLUMNS_PER_ROW
    {
        return Err(AppError::ResourceLimitExceeded(
            "sheet region exceeds row or column limits".to_string(),
        ));
    }
    Ok(())
}

fn sheet_layout_projection(sheet: &crate::types::SheetData) -> SheetLayoutProjection {
    SheetLayoutProjection {
        column_widths: sheet.column_widths.clone().unwrap_or_default(),
        row_heights: sheet.row_heights.clone().unwrap_or_default(),
    }
}

fn project_merge_anchor_cells(
    sheet: &crate::types::SheetData,
    region: &SheetRegion,
    merges: &[crate::types::MergeRange],
) -> Vec<crate::types::SheetCellChange> {
    let mut anchors = std::collections::BTreeSet::new();
    for merge in merges {
        let row = merge.start_row as usize;
        let col = merge.start_col as usize;
        if row >= region.row_start
            && row < region.row_end
            && col >= region.col_start
            && col < region.col_end
        {
            continue;
        }
        anchors.insert((row, col));
    }

    anchors
        .into_iter()
        .map(|(row, col)| {
            let value = sheet
                .rows
                .get(row)
                .and_then(|row_data| row_data.get(col))
                .cloned()
                .unwrap_or(crate::types::CellValue::Null);
            crate::types::SheetCellChange::new(region.sheet_index, row, col, value)
                .with_display_projection(
                    sheet.cell_display_text(row, col),
                    sheet.cell_format_at(row, col),
                    sheet.cell_style_at(row, col),
                )
        })
        .collect()
}

fn project_region_cells(
    sheet: &SheetData,
    region: &SheetRegion,
) -> Vec<crate::types::SheetCellChange> {
    let mut cells = Vec::new();
    for row_index in region.row_start..region.row_end {
        let Some(row) = sheet.rows.get(row_index) else {
            continue;
        };
        for (col_index, value) in row
            .iter()
            .enumerate()
            .take(region.col_end.min(row.len()))
            .skip(region.col_start)
        {
            let value = value.clone();
            cells.push(
                crate::types::SheetCellChange::new(region.sheet_index, row_index, col_index, value)
                    .with_display_projection(
                        sheet.cell_display_text(row_index, col_index),
                        sheet.cell_format_at(row_index, col_index),
                        sheet.cell_style_at(row_index, col_index),
                    ),
            );
        }
    }
    cells
}

fn native_save_extension(file_name: &str) -> Option<String> {
    if extension_of(file_name).is_none() {
        Some(default_extension_string())
    } else {
        supported_extension_from_name(file_name)
    }
}

fn export_extension(file_name: &str) -> Option<String> {
    native_save_extension(file_name)
}

fn document_format(file_name: &str) -> Option<String> {
    export_extension(file_name)
}

fn export_formats_for(_source_format: &str) -> Vec<String> {
    export_extensions()
}

fn default_extension_string() -> String {
    default_spreadsheet_extension().to_string()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock};

    use super::*;
    use crate::types::{CellValue, SheetData};

    #[test]
    fn document_capabilities_are_computed_by_backend() {
        assert_eq!(
            document_capabilities("book.xlsx", None),
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
            document_capabilities("data.csv", Some("/tmp/data.csv")),
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
    fn open_document_response_contains_manifest_and_initial_region() {
        let first_sheet = SheetData {
            name: "First".to_string(),
            rows: vec![vec![CellValue::String("loaded".to_string())]],
            ..Default::default()
        };
        let second_sheet = SheetData {
            name: "Second".to_string(),
            rows: vec![vec![CellValue::String("deferred".to_string())]],
            ..Default::default()
        };
        let state = EditorState::with_workbook(
            FileData {
                path: "/tmp/book.xlsx".to_string(),
                file_name: "book.xlsx".to_string(),
                sheets: vec![first_sheet, second_sheet],
            },
            None,
        );

        let response = finalize_open_document_response(open_document_response_snapshot(&state));

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
                crate::types::SheetExtent {
                    row_count: 1,
                    column_count: 1,
                },
                crate::types::SheetExtent {
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
        let sheet = SheetData {
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
        let region = SheetRegion {
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
            FileData {
                path: String::new(),
                file_name: "region.xlsx".to_string(),
                sheets: vec![SheetData {
                    rows: vec![vec![CellValue::String("value".to_string())]],
                    ..Default::default()
                }],
            },
            None,
        );
        let response = finalize_region_response(
            snapshot_sheet_region(
                &state,
                SheetRegion {
                    sheet_index: 0,
                    row_start: 0,
                    row_end: 1,
                    col_start: 0,
                    col_end: 1,
                },
            )
            .expect("region snapshot"),
            MAX_REGION_RESPONSE_BYTES,
        )
        .expect("region response");
        let serialized_bytes = serde_json::to_vec(&response)
            .expect("serialize response")
            .len();

        assert_eq!(response.estimated_bytes, Some(serialized_bytes));
        let mut unbounded = response;
        unbounded.estimated_bytes = None;
        assert!(matches!(
            finalize_region_response(unbounded, 1),
            Err(AppError::RegionResponseTooLarge {
                maximum_bytes: 1,
                ..
            })
        ));
    }

    #[test]
    fn region_snapshot_releases_document_lock_before_response_sizing() {
        let state = EditorState::with_workbook(
            FileData {
                path: String::new(),
                file_name: "region.xlsx".to_string(),
                sheets: vec![SheetData {
                    rows: vec![vec![CellValue::String("value".to_string())]],
                    ..Default::default()
                }],
            },
            None,
        );
        let document_id = state.document_id();
        let revision = state.revision();
        let mut store = crate::state::state::ActiveDocumentStore::new_for_test();
        store.replace_active_for_test(state);
        let registry = Arc::new(RwLock::new(store));

        let snapshot = sheet_region_snapshot_from_registry(
            &registry,
            document_id,
            revision,
            SheetRegion {
                sheet_index: 0,
                row_start: 0,
                row_end: 1,
                col_start: 0,
                col_end: 1,
            },
        )
        .expect("region snapshot");
        let write_guard = registry
            .try_write()
            .expect("region snapshot must not retain the document lock");

        let response =
            finalize_region_response(snapshot, MAX_REGION_RESPONSE_BYTES).expect("sized response");
        assert!(response.estimated_bytes.is_some());
        drop(write_guard);
    }

    #[test]
    fn region_projection_includes_merge_anchor_outside_region() {
        let sheet = SheetData {
            rows: vec![vec![CellValue::String("anchor".to_string())]],
            merges: vec![crate::types::MergeRange {
                start_row: 0,
                start_col: 0,
                end_row: 140,
                end_col: 0,
            }],
            ..Default::default()
        };
        let region = SheetRegion {
            sheet_index: 0,
            row_start: 128,
            row_end: 141,
            col_start: 0,
            col_end: 1,
        };
        let file_data = FileData {
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
        let region = SheetRegion {
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
            FileData {
                path: String::new(),
                file_name: "untitled.xlsx".to_string(),
                sheets: vec![SheetData::default(), SheetData::default()],
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
            FileData {
                path: "/tmp/data.csv".to_string(),
                file_name: "data.csv".to_string(),
                sheets: vec![SheetData::default()],
            },
            None,
        );

        assert!(ensure_native_save_target_allowed(&state, "/tmp/data.csv").is_ok());
    }
}
