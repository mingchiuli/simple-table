use crate::document_data::{DocumentData, DocumentSheet, RichMetadata};
use crate::document_layout_policy::{MAX_COLUMN_WIDTH_PX, MAX_ROW_HEIGHT_PX};
use std::collections::HashMap;

use crate::domain::cell_key::parse_cell_key;
use crate::domain::{
    AppliedOperation, CellValue, EditorCommand, ResolvedCellEdit, parse_cell_text,
};
use crate::error::AppError;
use crate::resource_limits::{
    ResourceLedger, validate_added_sheet, validate_column_width, validate_row_height,
};

impl EditorCommand {
    #[cfg(test)]
    pub fn resolve(self, file_data: &DocumentData) -> Result<AppliedOperation, AppError> {
        let resources = ResourceLedger::from_file_data(file_data);
        self.resolve_with_resources(file_data, &resources)
    }

    pub fn resolve_with_resources(
        self,
        file_data: &DocumentData,
        resources: &ResourceLedger,
    ) -> Result<AppliedOperation, AppError> {
        match self {
            EditorCommand::SetCell {
                sheet_index,
                row,
                col,
                text,
            } => {
                require_sheet(file_data, sheet_index)?;
                let old_value = file_data.sheets[sheet_index]
                    .rows
                    .get(row)
                    .and_then(|row_data| row_data.get(col))
                    .cloned()
                    .unwrap_or(CellValue::Null);
                let new_value = parse_cell_text(&text);
                if old_value != new_value {
                    resources.validate_cell_changes(
                        file_data,
                        [(sheet_index, row, col, &old_value, &new_value)],
                    )?;
                }
                Ok(AppliedOperation::SetCell {
                    sheet_index,
                    row,
                    col,
                    old_value,
                    new_value,
                })
            }
            EditorCommand::SetCells { changes } => {
                if changes.is_empty() {
                    return Ok(AppliedOperation::SetCells {
                        changes: Vec::new(),
                    });
                }
                let mut resolved: Vec<ResolvedCellEdit> = Vec::with_capacity(changes.len());
                let mut positions: HashMap<(usize, usize, usize), usize> = HashMap::new();
                for change in changes {
                    require_sheet(file_data, change.sheet_index)?;
                    let key = (change.sheet_index, change.row, change.col);
                    let new_value = parse_cell_text(&change.text);
                    if let Some(index) = positions.get(&key) {
                        resolved[*index].new_value = new_value;
                    } else {
                        let old_value = file_data.sheets[change.sheet_index]
                            .rows
                            .get(change.row)
                            .and_then(|row_data| row_data.get(change.col))
                            .cloned()
                            .unwrap_or(CellValue::Null);
                        positions.insert(key, resolved.len());
                        resolved.push(ResolvedCellEdit {
                            sheet_index: change.sheet_index,
                            row: change.row,
                            col: change.col,
                            old_value,
                            new_value,
                        });
                    }
                }
                resolved.retain(|change| change.old_value != change.new_value);
                if !resolved.is_empty() {
                    resources.validate_cell_changes(
                        file_data,
                        resolved.iter().map(|change| {
                            (
                                change.sheet_index,
                                change.row,
                                change.col,
                                &change.old_value,
                                &change.new_value,
                            )
                        }),
                    )?;
                }
                Ok(AppliedOperation::SetCells { changes: resolved })
            }
            EditorCommand::AddRow {
                sheet_index,
                row_index,
            } => {
                let sheet = require_sheet(file_data, sheet_index)?;
                let extent = SheetMutationExtent::from_sheet(sheet);
                if row_index > extent.rows {
                    return Err(AppError::RowNotFound(row_index));
                }
                resources.validate_added_row(
                    sheet,
                    sheet.rows.len().max(row_index).saturating_add(1),
                    extent.columns,
                )?;
                Ok(AppliedOperation::AddRow {
                    sheet_index,
                    row_index,
                    row_data: vec![CellValue::Null; extent.columns],
                    row_height: None,
                })
            }
            EditorCommand::DeleteRow {
                sheet_index,
                row_index,
            } => {
                let sheet = require_sheet(file_data, sheet_index)?;
                let extent = SheetMutationExtent::from_sheet(sheet);
                if row_index >= extent.rows {
                    return Err(AppError::RowNotFound(row_index));
                }
                Ok(AppliedOperation::DeleteRow {
                    sheet_index,
                    row_index,
                })
            }
            EditorCommand::AddColumn {
                sheet_index,
                col_index,
            } => {
                let sheet = require_sheet(file_data, sheet_index)?;
                let extent = SheetMutationExtent::from_sheet(sheet);
                if col_index > extent.columns {
                    return Err(AppError::InvalidCellPosition {
                        row: 0,
                        col: col_index,
                    });
                }
                resources.validate_added_column(sheet, extent.rows, col_index)?;
                Ok(AppliedOperation::AddColumn {
                    sheet_index,
                    col_index,
                    col_data: vec![CellValue::Null; extent.rows],
                    column_width: None,
                })
            }
            EditorCommand::DeleteColumn {
                sheet_index,
                col_index,
            } => {
                let sheet = require_sheet(file_data, sheet_index)?;
                let extent = SheetMutationExtent::from_sheet(sheet);
                if col_index >= extent.columns {
                    return Err(AppError::InvalidCellPosition {
                        row: 0,
                        col: col_index,
                    });
                }
                Ok(AppliedOperation::DeleteColumn {
                    sheet_index,
                    col_index,
                })
            }
            EditorCommand::SetColumnWidth {
                sheet_index,
                col_index,
                width,
            } => {
                validate_column_width(width)?;
                let sheet = require_sheet(file_data, sheet_index)?;
                let extent = SheetMutationExtent::from_sheet(sheet);
                if col_index >= extent.resizable_columns() {
                    return Err(AppError::InvalidCellPosition {
                        row: 0,
                        col: col_index,
                    });
                }
                let old_width = sheet
                    .column_widths
                    .as_ref()
                    .and_then(|widths| widths.get(&col_index).copied());
                resources.validate_layout_change(old_width.is_some(), width.is_some())?;
                Ok(AppliedOperation::SetColumnWidth {
                    sheet_index,
                    col_index,
                    old_width,
                    new_width: width,
                })
            }
            EditorCommand::SetRowHeight {
                sheet_index,
                row_index,
                height,
            } => {
                validate_row_height(height)?;
                let sheet = require_sheet(file_data, sheet_index)?;
                let extent = SheetMutationExtent::from_sheet(sheet);
                if row_index >= extent.resizable_rows() {
                    return Err(AppError::RowNotFound(row_index));
                }
                let old_height = sheet
                    .row_heights
                    .as_ref()
                    .and_then(|heights| heights.get(&row_index).copied());
                resources.validate_layout_change(old_height.is_some(), height.is_some())?;
                Ok(AppliedOperation::SetRowHeight {
                    sheet_index,
                    row_index,
                    old_height,
                    new_height: height,
                })
            }
            EditorCommand::AddSheet { name } => {
                let sheet_index = file_data.sheets.len();
                let sheet_name = name
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| format!("Sheet{}", sheet_index + 1));
                validate_added_sheet(file_data, &sheet_name)?;
                Ok(AppliedOperation::AddSheet {
                    sheet_index,
                    name: sheet_name,
                    row_count: 5,
                    column_count: 5,
                })
            }
            EditorCommand::DeleteSheet { sheet_index } => {
                if file_data.sheets.len() <= 1 {
                    return Err(AppError::CannotDeleteLastSheet);
                }
                require_sheet(file_data, sheet_index)?;
                Ok(AppliedOperation::DeleteSheet { sheet_index })
            }
            EditorCommand::InsertImage {
                sheet_index,
                mut image,
                image_name,
                bytes,
            } => {
                validate_image_anchor(&image.anchor)?;
                let sheet = file_data
                    .sheets
                    .get(sheet_index)
                    .ok_or(AppError::InvalidSheetIndex(sheet_index))?;
                if sheet
                    .rich
                    .images
                    .iter()
                    .any(|existing| existing.id == image.id)
                {
                    return Err(AppError::DocumentStateInvalid(format!(
                        "image {} already exists",
                        image.id
                    )));
                }
                image.z_index = sheet.rich.images.len();
                let (column_width, row_height) = exact_fit_layout(&image);
                Ok(AppliedOperation::InsertImage {
                    sheet_index,
                    image,
                    image_name,
                    bytes,
                    column_width,
                    row_height,
                })
            }
            EditorCommand::UpdateImage {
                sheet_index,
                image_id,
                anchor,
            } => {
                validate_image_anchor(&anchor)?;
                let sheet = file_data
                    .sheets
                    .get(sheet_index)
                    .ok_or(AppError::InvalidSheetIndex(sheet_index))?;
                let old_image = sheet
                    .rich
                    .images
                    .iter()
                    .find(|image| image.id == image_id)
                    .cloned()
                    .ok_or_else(|| {
                        AppError::DocumentStateInvalid(format!("image {image_id} does not exist"))
                    })?;
                if !old_image.renderable {
                    return Err(AppError::DocumentStateInvalid(format!(
                        "image {image_id} uses an unsupported format"
                    )));
                }
                let mut new_image = old_image.clone();
                new_image.anchor = anchor;
                Ok(AppliedOperation::UpdateImage {
                    sheet_index,
                    old_image,
                    new_image,
                })
            }
            EditorCommand::DeleteImage {
                sheet_index,
                image_id,
            } => {
                let sheet = file_data
                    .sheets
                    .get(sheet_index)
                    .ok_or(AppError::InvalidSheetIndex(sheet_index))?;
                let image = sheet
                    .rich
                    .images
                    .iter()
                    .find(|image| image.id == image_id)
                    .cloned()
                    .ok_or_else(|| {
                        AppError::DocumentStateInvalid(format!("image {image_id} does not exist"))
                    })?;
                Ok(AppliedOperation::DeleteImage { sheet_index, image })
            }
        }
    }
}

/// Computes the exact-fit column width / row height (pixels) for a newly
/// inserted one-cell image so the containing cell matches the image's display
/// size. Insert always anchors images with `OneCell`; `TwoCell` anchors are
/// left untouched (the cell is sized once, at insert).
fn exact_fit_layout(image: &crate::document_data::SheetImage) -> (Option<u32>, Option<u32>) {
    const EMU_PER_PIXEL: i64 = 9_525;
    match &image.anchor {
        crate::document_data::ImageAnchor::OneCell {
            from,
            width_emu,
            height_emu,
        } => {
            let column_width = ((f64::from(from.col_offset_emu) + *width_emu as f64)
                / EMU_PER_PIXEL as f64)
                .ceil()
                .clamp(1.0, MAX_COLUMN_WIDTH_PX as f64) as u32;
            let row_height = ((f64::from(from.row_offset_emu) + *height_emu as f64)
                / EMU_PER_PIXEL as f64)
                .ceil()
                .clamp(1.0, MAX_ROW_HEIGHT_PX as f64) as u32;
            (Some(column_width), Some(row_height))
        }
        crate::document_data::ImageAnchor::TwoCell { .. } => (None, None),
    }
}

fn validate_image_anchor(anchor: &crate::document_data::ImageAnchor) -> Result<(), AppError> {
    const MAX_ROW: u32 = 1_048_575;
    const MAX_COL: u32 = 16_383;
    const MAX_IMAGE_EMU: i64 = 100_000 * 9_525;
    let validate_marker = |marker: &crate::document_data::ImageMarker| {
        if marker.row > MAX_ROW || marker.col > MAX_COL {
            return Err(AppError::InvalidCellPosition {
                row: marker.row as usize,
                col: marker.col as usize,
            });
        }
        if marker.row_offset_emu < 0 || marker.col_offset_emu < 0 {
            return Err(AppError::DocumentStateInvalid(
                "image anchor offsets cannot be negative".to_string(),
            ));
        }
        Ok(())
    };
    match anchor {
        crate::document_data::ImageAnchor::OneCell {
            from,
            width_emu,
            height_emu,
        } => {
            validate_marker(from)?;
            if !(1..=MAX_IMAGE_EMU).contains(width_emu) || !(1..=MAX_IMAGE_EMU).contains(height_emu)
            {
                return Err(AppError::ResourceLimitExceeded(
                    "image dimensions are outside the supported range".to_string(),
                ));
            }
        }
        crate::document_data::ImageAnchor::TwoCell { from, to } => {
            validate_marker(from)?;
            validate_marker(to)?;
            let width_is_positive = to.col > from.col
                || (to.col == from.col && to.col_offset_emu > from.col_offset_emu);
            let height_is_positive = to.row > from.row
                || (to.row == from.row && to.row_offset_emu > from.row_offset_emu);
            if !width_is_positive || !height_is_positive {
                return Err(AppError::DocumentStateInvalid(
                    "two-cell image anchor must have positive width and height".to_string(),
                ));
            }
        }
    }
    Ok(())
}

fn require_sheet(file_data: &DocumentData, sheet_index: usize) -> Result<&DocumentSheet, AppError> {
    file_data
        .sheets
        .get(sheet_index)
        .ok_or(AppError::InvalidSheetIndex(sheet_index))
}

struct SheetMutationExtent {
    rows: usize,
    columns: usize,
}

impl SheetMutationExtent {
    fn from_sheet(sheet: &DocumentSheet) -> Self {
        let value_rows = sheet.rows.len();
        let value_columns = sheet.rows.iter().map(Vec::len).max().unwrap_or(0);
        let merge_rows = sheet
            .merges
            .iter()
            .map(|merge| merge.end_row as usize + 1)
            .max()
            .unwrap_or(0);
        let merge_columns = sheet
            .merges
            .iter()
            .map(|merge| merge.end_col as usize + 1)
            .max()
            .unwrap_or(0);
        let layout_rows = sheet
            .row_heights
            .as_ref()
            .and_then(|heights| heights.keys().max().map(|index| index + 1))
            .unwrap_or(0);
        let layout_columns = sheet
            .column_widths
            .as_ref()
            .and_then(|widths| widths.keys().max().map(|index| index + 1))
            .unwrap_or(0);
        let rich = rich_projection_extent(&sheet.rich);

        Self {
            rows: value_rows.max(merge_rows).max(layout_rows).max(rich.rows),
            columns: value_columns
                .max(merge_columns)
                .max(layout_columns)
                .max(rich.columns),
        }
    }

    fn resizable_rows(&self) -> usize {
        self.rows.max(1)
    }

    fn resizable_columns(&self) -> usize {
        self.columns.max(1)
    }
}

fn rich_projection_extent(rich: &RichMetadata) -> SheetMutationExtent {
    let mut rows = 0;
    let mut columns = 0;

    for key in rich
        .cell_formats
        .keys()
        .chain(rich.cell_styles.keys())
        .chain(rich.hyperlinks.keys())
    {
        if let Some((row, col)) = parse_cell_key(key) {
            rows = rows.max(row + 1);
            columns = columns.max(col + 1);
        }
    }

    rows = rows.max(
        rich.hidden_rows
            .iter()
            .copied()
            .max()
            .map(|row| row + 1)
            .unwrap_or(0),
    );
    columns = columns.max(
        rich.hidden_columns
            .iter()
            .copied()
            .max()
            .map(|col| col + 1)
            .unwrap_or(0),
    );

    for drawing in &rich.drawings {
        rows = rows.max(
            (drawing
                .to_row
                .unwrap_or(drawing.from_row)
                .max(drawing.from_row) as usize)
                + 1,
        );
        columns = columns.max(
            (drawing
                .to_col
                .unwrap_or(drawing.from_col)
                .max(drawing.from_col) as usize)
                + 1,
        );
    }

    SheetMutationExtent { rows, columns }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_data::{CellStyle, Drawing, DrawingKind, Hyperlink};
    use std::sync::Arc;
    fn file_data_with_rich(rich: RichMetadata) -> DocumentData {
        DocumentData {
            path: String::new(),
            file_name: "rich.xlsx".to_string(),
            sheets: vec![DocumentSheet {
                name: "Sheet1".to_string(),
                rows: Vec::new(),
                rich,
                ..Default::default()
            }],
        }
    }

    #[test]
    fn sheet_extent_includes_rich_cell_metadata() {
        let file_data = file_data_with_rich(RichMetadata {
            cell_styles: [(
                "E4".to_string(),
                CellStyle {
                    bold: Some(true),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
            hyperlinks: [(
                "F5".to_string(),
                Hyperlink {
                    url: "https://example.com".to_string(),
                    tooltip: None,
                    location: false,
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        });

        assert!(
            matches!(
                EditorCommand::SetRowHeight {
                    sheet_index: 0,
                    row_index: 4,
                    height: Some(80),
                }
                .resolve(&file_data),
                Ok(AppliedOperation::SetRowHeight { row_index: 4, .. })
            ),
            "row extent should include hyperlink/style-only rows"
        );
        assert!(
            matches!(
                EditorCommand::SetColumnWidth {
                    sheet_index: 0,
                    col_index: 5,
                    width: Some(120),
                }
                .resolve(&file_data),
                Ok(AppliedOperation::SetColumnWidth { col_index: 5, .. })
            ),
            "column extent should include hyperlink/style-only columns"
        );
    }

    #[test]
    fn layout_mutations_reject_dimensions_outside_the_domain_policy() {
        let file_data = DocumentData {
            path: String::new(),
            file_name: "book.xlsx".to_string(),
            sheets: vec![DocumentSheet {
                rows: vec![vec![CellValue::Null]],
                ..Default::default()
            }],
        };

        assert!(matches!(
            EditorCommand::SetColumnWidth {
                sheet_index: 0,
                col_index: 0,
                width: Some(0),
            }
            .resolve(&file_data),
            Err(AppError::ResourceLimitExceeded(_))
        ));
        assert!(matches!(
            EditorCommand::SetRowHeight {
                sheet_index: 0,
                row_index: 0,
                height: Some(crate::document_layout_policy::MAX_ROW_HEIGHT_PX + 1),
            }
            .resolve(&file_data),
            Err(AppError::ResourceLimitExceeded(_))
        ));
    }

    #[test]
    fn sheet_extent_includes_hidden_rows_columns_and_drawings() {
        let file_data = file_data_with_rich(RichMetadata {
            hidden_rows: vec![9],
            hidden_columns: vec![7],
            drawings: vec![Drawing {
                kind: DrawingKind::Chart,
                from_row: 11,
                from_col: 12,
                to_row: Some(14),
                to_col: Some(15),
            }],
            ..Default::default()
        });

        assert!(matches!(
            EditorCommand::DeleteRow {
                sheet_index: 0,
                row_index: 14,
            }
            .resolve(&file_data),
            Ok(AppliedOperation::DeleteRow { row_index: 14, .. })
        ));
        assert!(matches!(
            EditorCommand::DeleteColumn {
                sheet_index: 0,
                col_index: 15,
            }
            .resolve(&file_data),
            Ok(AppliedOperation::DeleteColumn { col_index: 15, .. })
        ));
    }

    #[test]
    fn insert_image_resolves_to_exact_fit_layout() {
        let file_data = DocumentData {
            path: String::new(),
            file_name: "images.xlsx".to_string(),
            sheets: vec![DocumentSheet {
                name: "Sheet1".to_string(),
                ..Default::default()
            }],
        };
        let image = crate::document_data::SheetImage {
            id: "image-1".to_string(),
            media_id: "media".to_string(),
            mime_type: "image/png".to_string(),
            intrinsic_width: 2,
            intrinsic_height: 1,
            anchor: crate::document_data::ImageAnchor::OneCell {
                from: crate::document_data::ImageMarker {
                    row: 1,
                    col: 2,
                    ..Default::default()
                },
                width_emu: 200 * 9_525,
                height_emu: 150 * 9_525,
            },
            z_index: 0,
            renderable: true,
        };
        let resolved = EditorCommand::InsertImage {
            sheet_index: 0,
            image,
            image_name: "test.png".to_string(),
            bytes: Arc::from(Vec::<u8>::new()),
        }
        .resolve(&file_data)
        .expect("resolve");

        match resolved {
            AppliedOperation::InsertImage {
                column_width,
                row_height,
                ..
            } => {
                assert_eq!(column_width, Some(200));
                assert_eq!(row_height, Some(150));
            }
            other => panic!("expected InsertImage, got {other:?}"),
        }
    }
}
