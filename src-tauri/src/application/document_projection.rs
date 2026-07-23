use crate::document::document_model::SpreadsheetDocument;
use crate::document::region_metadata_index::DocumentRegion;
use crate::document_data::{DocumentSheet, MergeRange, SheetExtent};
use crate::editor_protocol::{SHEET_REGION_TILE_COLUMNS, SHEET_REGION_TILE_ROWS};
use crate::error::AppError;
use crate::projection_model::{
    DocumentManifestSnapshot, EditorSessionSnapshot, EditorStateSnapshot, OpenDocumentSnapshot,
    ProjectedCellChange, SavedDocumentIdentity, SavedDocumentOutcome, SheetLayoutSnapshot,
    SheetManifestSnapshot, SheetRegionSnapshot,
};
use crate::state::editor_state::EditorState;

const MAX_REGION_CELLS: usize = 65_536;
pub(crate) const MAX_REGION_ROWS: usize = 1_024;
const MAX_REGION_COLUMNS: usize = 512;

pub(crate) fn editor_session_snapshot(editor_state: &EditorState) -> EditorSessionSnapshot {
    EditorSessionSnapshot {
        document_id: editor_state.document_id(),
        revision: editor_state.revision(),
        formula_status: editor_state.formula_status(),
        capabilities: editor_state.capabilities(),
        editor_state: EditorStateSnapshot {
            can_undo: editor_state.can_undo(),
            can_redo: editor_state.can_redo(),
            is_dirty: editor_state.is_dirty(),
            history: editor_state.history_status(),
        },
    }
}

pub(crate) fn open_document_snapshot(editor_state: &EditorState) -> OpenDocumentSnapshot {
    open_document_snapshot_for_sheet(editor_state, 0)
}

pub(crate) fn open_document_snapshot_for_sheet(
    editor_state: &EditorState,
    preferred_sheet_index: usize,
) -> OpenDocumentSnapshot {
    let initial_region = editor_state
        .sheet_extent(preferred_sheet_index)
        .map(|extent| initial_sheet_region(preferred_sheet_index, &extent))
        .and_then(|region| snapshot_sheet_region(editor_state, region).ok());
    OpenDocumentSnapshot {
        document: document_manifest(editor_state),
        editor_session: editor_session_snapshot(editor_state),
        initial_region,
    }
}

pub(crate) fn document_manifest(editor_state: &EditorState) -> DocumentManifestSnapshot {
    let source = editor_state.file_data();
    let extents = editor_state.sheet_extents();
    DocumentManifestSnapshot {
        path: source.path.clone(),
        file_name: source.file_name.clone(),
        sheets: source
            .sheets
            .iter()
            .zip(extents)
            .map(|(sheet, extent)| SheetManifestSnapshot {
                name: sheet.name.clone(),
                extent,
                layout: sheet_layout_projection(sheet),
            })
            .collect(),
    }
}

pub(crate) fn saved_document_outcome_with_reparse(
    editor_state: &EditorState,
    saved_document: &SpreadsheetDocument,
    clear_history: bool,
) -> Result<SavedDocumentOutcome, AppError> {
    Ok(SavedDocumentOutcome {
        document: Some(document_manifest_from_data(saved_document.projection())),
        identity: None,
        editor_session: saved_editor_session_snapshot(
            editor_state,
            Some(saved_document),
            clear_history,
        )?,
    })
}

pub(crate) fn saved_document_outcome_without_reparse(
    editor_state: &EditorState,
    path: String,
    file_name: String,
    clear_history: bool,
) -> Result<SavedDocumentOutcome, AppError> {
    Ok(SavedDocumentOutcome {
        document: None,
        identity: Some(SavedDocumentIdentity { path, file_name }),
        editor_session: saved_editor_session_snapshot(editor_state, None, clear_history)?,
    })
}

fn saved_editor_session_snapshot(
    editor_state: &EditorState,
    saved_document: Option<&SpreadsheetDocument>,
    clear_history: bool,
) -> Result<EditorSessionSnapshot, AppError> {
    let revision = editor_state.revision().checked_add(1).ok_or_else(|| {
        AppError::DocumentStateInvalid("document revision space exhausted".to_string())
    })?;
    let mut history = editor_state.history_status();
    if clear_history {
        history.is_truncated = false;
        history.reason = None;
        history.undo_entries = 0;
        history.redo_entries = 0;
        history.undo_estimated_bytes = 0;
        history.redo_estimated_bytes = 0;
    }
    let transaction_available = saved_document
        .map(|document| document.transaction_failure().is_none())
        .unwrap_or_else(|| editor_state.transaction_failure().is_none());
    Ok(EditorSessionSnapshot {
        document_id: editor_state.document_id(),
        revision,
        formula_status: saved_document
            .map(SpreadsheetDocument::formula_status)
            .unwrap_or_else(|| editor_state.formula_status()),
        capabilities: saved_document
            .map(SpreadsheetDocument::capabilities)
            .unwrap_or_else(|| editor_state.capabilities()),
        editor_state: EditorStateSnapshot {
            can_undo: transaction_available && !clear_history && history.undo_entries > 0,
            can_redo: transaction_available && !clear_history && history.redo_entries > 0,
            is_dirty: false,
            history,
        },
    })
}

fn document_manifest_from_data(
    source: &crate::document_data::DocumentData,
) -> DocumentManifestSnapshot {
    DocumentManifestSnapshot {
        path: source.path.clone(),
        file_name: source.file_name.clone(),
        sheets: source
            .sheets
            .iter()
            .map(|sheet| SheetManifestSnapshot {
                name: sheet.name.clone(),
                extent: sheet.extent(),
                layout: sheet_layout_projection(sheet),
            })
            .collect(),
    }
}

pub(crate) fn snapshot_sheet_region(
    editor_state: &EditorState,
    region: DocumentRegion,
) -> Result<SheetRegionSnapshot, AppError> {
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
    Ok(SheetRegionSnapshot {
        document_id: editor_state.document_id(),
        revision: editor_state.revision(),
        region,
        cells,
        merge_anchor_cells,
        metadata,
    })
}

pub(crate) fn validate_sheet_region(region: &DocumentRegion) -> Result<(), AppError> {
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

fn initial_sheet_region(sheet_index: usize, extent: &SheetExtent) -> DocumentRegion {
    DocumentRegion {
        sheet_index,
        row_start: 0,
        row_end: extent.row_count.min(SHEET_REGION_TILE_ROWS),
        col_start: 0,
        col_end: extent.column_count.min(SHEET_REGION_TILE_COLUMNS),
    }
}

fn sheet_layout_projection(sheet: &DocumentSheet) -> SheetLayoutSnapshot {
    SheetLayoutSnapshot {
        column_widths: sheet.column_widths.clone().unwrap_or_default(),
        row_heights: sheet.row_heights.clone().unwrap_or_default(),
    }
}

pub(crate) fn project_merge_anchor_cells(
    sheet: &DocumentSheet,
    region: &DocumentRegion,
    merges: &[MergeRange],
) -> Vec<ProjectedCellChange> {
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
                .unwrap_or(crate::domain::CellValue::Null);
            ProjectedCellChange::new(region.sheet_index, row, col, value).with_display_projection(
                sheet.cell_display_text(row, col),
                sheet.cell_format_at(row, col),
                sheet.cell_style_at(row, col),
            )
        })
        .collect()
}

pub(crate) fn project_region_cells(
    sheet: &DocumentSheet,
    region: &DocumentRegion,
) -> Vec<ProjectedCellChange> {
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
                ProjectedCellChange::new(region.sheet_index, row_index, col_index, value.clone())
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
