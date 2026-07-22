use crate::document::region_metadata_index::{DocumentRegion, DocumentRegionMetadata};
use crate::document_data::{CellFormat, CellStyle, MergeRange, SheetExtent};
use crate::projection_model::{ProjectedCellChange, SheetLayoutSnapshot, SheetManifestSnapshot};
use crate::types;

pub(super) fn region_metadata(value: DocumentRegionMetadata) -> types::SheetRegionMetadata {
    types::SheetRegionMetadata {
        merges: value.merges.into_iter().map(merge_range).collect(),
        cell_formats: value
            .cell_formats
            .into_iter()
            .map(|(key, value)| (key, cell_format(value)))
            .collect(),
        cell_styles: value
            .cell_styles
            .into_iter()
            .map(|(key, value)| (key, cell_style(value)))
            .collect(),
    }
}

fn cell_format(value: CellFormat) -> types::CellFormatProjection {
    types::CellFormatProjection {
        number_format: value.number_format,
        style_id: value.style_id,
    }
}

fn merge_range(value: MergeRange) -> types::MergeRange {
    types::MergeRange {
        start_row: value.start_row,
        start_col: value.start_col,
        end_row: value.end_row,
        end_col: value.end_col,
    }
}

fn cell_style(value: CellStyle) -> types::CellStyleProjection {
    types::CellStyleProjection {
        font_color: value.font_color,
        background_color: value.background_color,
        bold: value.bold,
        italic: value.italic,
        horizontal_align: value.horizontal_align,
        vertical_align: value.vertical_align,
        number_format: value.number_format,
    }
}

pub(super) fn sheet_manifest(value: SheetManifestSnapshot) -> types::SheetManifest {
    types::SheetManifest {
        name: value.name,
        extent: sheet_extent(value.extent),
        layout: sheet_layout(value.layout),
    }
}

pub(super) fn sheet_extent(value: SheetExtent) -> types::SheetExtent {
    types::SheetExtent {
        row_count: value.row_count,
        column_count: value.column_count,
    }
}

fn sheet_layout(value: SheetLayoutSnapshot) -> types::SheetLayoutProjection {
    types::SheetLayoutProjection {
        column_widths: value.column_widths,
        row_heights: value.row_heights,
    }
}

pub(super) fn sheet_region(value: DocumentRegion) -> types::SheetRegion {
    types::SheetRegion {
        sheet_index: value.sheet_index,
        row_start: value.row_start,
        row_end: value.row_end,
        col_start: value.col_start,
        col_end: value.col_end,
    }
}

pub(super) fn projected_cell_change(value: ProjectedCellChange) -> types::SheetCellChange {
    let format = value.format.map(cell_format);
    let style = value.style.map(cell_style);
    let display = value
        .display
        .unwrap_or_else(|| value.value.to_display_string());
    types::SheetCellChange::new(value.sheet_index, value.row, value.col, value.value)
        .with_display_projection(display, format, style)
}
