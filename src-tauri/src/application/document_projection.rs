use crate::document_data::{
    CellFormat, CellStyle, DocumentSheet, SheetExtent as DocumentSheetExtent,
};
use crate::error::AppError;
use crate::ops::patch_projector::editor_state_info;
use crate::state::editor_state::EditorState;
use crate::types::{
    CellFormatProjection, CellStyleProjection, DocumentManifest, EditorSessionInfo,
    OpenDocumentResponse, SheetExtent, SheetLayoutProjection, SheetManifest, SheetRegion,
    SheetRegionProjectionResponse,
};

const INITIAL_REGION_ROWS: usize = 128;
const INITIAL_REGION_COLUMNS: usize = 32;
const MAX_REGION_CELLS: usize = 65_536;
pub(crate) const MAX_REGION_ROWS: usize = 1_024;
const MAX_REGION_COLUMNS: usize = 512;

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

pub(crate) fn open_document_response_snapshot_for_sheet(
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
                extent: project_sheet_extent(extent),
                layout: sheet_layout_projection(sheet),
            })
            .collect(),
    }
}

pub(crate) fn snapshot_sheet_region(
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

pub(crate) fn validate_sheet_region(region: &SheetRegion) -> Result<(), AppError> {
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
    if region.row_end > crate::resource_limits::MAX_ROWS_PER_SHEET
        || region.col_end > crate::resource_limits::MAX_COLUMNS_PER_ROW
    {
        return Err(AppError::ResourceLimitExceeded(
            "sheet region exceeds row or column limits".to_string(),
        ));
    }
    Ok(())
}

fn initial_sheet_region(sheet_index: usize, extent: &DocumentSheetExtent) -> SheetRegion {
    SheetRegion {
        sheet_index,
        row_start: 0,
        row_end: extent.row_count.min(INITIAL_REGION_ROWS),
        col_start: 0,
        col_end: extent.column_count.min(INITIAL_REGION_COLUMNS),
    }
}

fn sheet_layout_projection(sheet: &DocumentSheet) -> SheetLayoutProjection {
    SheetLayoutProjection {
        column_widths: sheet.column_widths.clone().unwrap_or_default(),
        row_heights: sheet.row_heights.clone().unwrap_or_default(),
    }
}

pub(crate) fn project_merge_anchor_cells(
    sheet: &DocumentSheet,
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
                    sheet.cell_format_at(row, col).map(project_cell_format),
                    sheet.cell_style_at(row, col).map(project_cell_style),
                )
        })
        .collect()
}

pub(crate) fn project_region_cells(
    sheet: &DocumentSheet,
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
            cells.push(
                crate::types::SheetCellChange::new(
                    region.sheet_index,
                    row_index,
                    col_index,
                    value.clone(),
                )
                .with_display_projection(
                    sheet.cell_display_text(row_index, col_index),
                    sheet
                        .cell_format_at(row_index, col_index)
                        .map(project_cell_format),
                    sheet
                        .cell_style_at(row_index, col_index)
                        .map(project_cell_style),
                ),
            );
        }
    }
    cells
}

fn project_sheet_extent(value: DocumentSheetExtent) -> SheetExtent {
    SheetExtent {
        row_count: value.row_count,
        column_count: value.column_count,
    }
}

pub(crate) fn project_cell_format(value: CellFormat) -> CellFormatProjection {
    CellFormatProjection {
        number_format: value.number_format,
        style_id: value.style_id,
    }
}

pub(crate) fn project_cell_style(value: CellStyle) -> CellStyleProjection {
    CellStyleProjection {
        font_color: value.font_color,
        background_color: value.background_color,
        bold: value.bold,
        italic: value.italic,
        horizontal_align: value.horizontal_align,
        vertical_align: value.vertical_align,
        number_format: value.number_format,
    }
}
