use std::collections::HashMap;

use crate::domain::{CellValue, format_cell_display, format_cell_search};

pub const MAX_EMBEDDED_IMAGE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_RENDER_IMAGE_PIXELS: u64 = 40_000_000;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CellFormat {
    pub number_format: Option<String>,
    pub style_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CellStyle {
    pub font_color: Option<String>,
    pub background_color: Option<String>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub horizontal_align: Option<String>,
    pub vertical_align: Option<String>,
    pub number_format: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FreezePane {
    pub top_left_cell: String,
    pub horizontal_split: f64,
    pub vertical_split: f64,
    pub active_pane: String,
    pub state: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hyperlink {
    pub url: String,
    pub tooltip: Option<String>,
    pub location: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DrawingKind {
    Chart,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drawing {
    pub kind: DrawingKind,
    pub from_row: u32,
    pub from_col: u32,
    pub to_row: Option<u32>,
    pub to_col: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageMarker {
    pub row: u32,
    pub col: u32,
    pub row_offset_emu: i32,
    pub col_offset_emu: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageAnchor {
    OneCell {
        from: ImageMarker,
        width_emu: i64,
        height_emu: i64,
    },
    TwoCell {
        from: ImageMarker,
        to: ImageMarker,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetImage {
    pub id: String,
    pub media_id: String,
    pub mime_type: String,
    pub intrinsic_width: u32,
    pub intrinsic_height: u32,
    pub anchor: ImageAnchor,
    pub z_index: usize,
    pub renderable: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichMetadata {
    pub cell_formats: HashMap<String, CellFormat>,
    pub cell_styles: HashMap<String, CellStyle>,
    pub hidden_rows: Vec<usize>,
    pub hidden_columns: Vec<usize>,
    pub freeze_pane: Option<FreezePane>,
    pub hyperlinks: HashMap<String, Hyperlink>,
    pub drawings: Vec<Drawing>,
    pub images: Vec<SheetImage>,
    pub has_more_drawings: bool,
    pub has_style_metadata: bool,
    pub has_hyperlinks: bool,
    pub has_freeze_pane: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeRange {
    pub start_row: u32,
    pub start_col: u16,
    pub end_row: u32,
    pub end_col: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SheetExtent {
    pub row_count: usize,
    pub column_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DocumentSheet {
    pub name: String,
    pub rows: Vec<Vec<CellValue>>,
    pub merges: Vec<MergeRange>,
    pub column_widths: Option<HashMap<usize, u32>>,
    pub row_heights: Option<HashMap<usize, u32>>,
    pub rich: RichMetadata,
}

impl DocumentSheet {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn search_snapshot(&self) -> Self {
        Self {
            name: self.name.clone(),
            rows: self.rows.clone(),
            merges: Vec::new(),
            column_widths: None,
            row_heights: None,
            rich: RichMetadata {
                cell_formats: self.rich.cell_formats.clone(),
                cell_styles: self.rich.cell_styles.clone(),
                ..Default::default()
            },
        }
    }

    pub fn extent(&self) -> SheetExtent {
        let value_row_count = self.rows.len();
        let value_column_count = self.rows.iter().map(Vec::len).max().unwrap_or(0);
        let merge_row_count = self
            .merges
            .iter()
            .map(|merge| merge.end_row as usize + 1)
            .max()
            .unwrap_or(0);
        let merge_column_count = self
            .merges
            .iter()
            .map(|merge| merge.end_col as usize + 1)
            .max()
            .unwrap_or(0);
        let layout_row_count = self
            .row_heights
            .as_ref()
            .and_then(|values| values.keys().max().map(|index| index + 1))
            .unwrap_or(0);
        let layout_column_count = self
            .column_widths
            .as_ref()
            .and_then(|values| values.keys().max().map(|index| index + 1))
            .unwrap_or(0);
        let rich_extent = rich_projection_extent(&self.rich);

        SheetExtent {
            row_count: value_row_count
                .max(merge_row_count)
                .max(layout_row_count)
                .max(rich_extent.row_count),
            column_count: value_column_count
                .max(merge_column_count)
                .max(layout_column_count)
                .max(rich_extent.column_count),
        }
    }

    pub fn cell_format_at(&self, row: usize, col: usize) -> Option<CellFormat> {
        let key = excel_cell_key(row, col);
        let explicit = self.rich.cell_formats.get(&key);
        let style_number_format = self
            .rich
            .cell_styles
            .get(&key)
            .and_then(|style| style.number_format.clone());

        if explicit.is_none() && style_number_format.is_none() {
            return None;
        }

        Some(CellFormat {
            number_format: explicit
                .and_then(|format| format.number_format.clone())
                .or(style_number_format),
            style_id: explicit.and_then(|format| format.style_id.clone()),
        })
    }

    pub fn cell_style_at(&self, row: usize, col: usize) -> Option<CellStyle> {
        self.rich
            .cell_styles
            .get(&excel_cell_key(row, col))
            .cloned()
    }

    pub fn cell_display_text(&self, row: usize, col: usize) -> String {
        self.rows
            .get(row)
            .and_then(|row_data| row_data.get(col))
            .map(|cell| {
                let format = self.cell_format_at(row, col);
                format_cell_display(
                    cell,
                    format.and_then(|value| value.number_format).as_deref(),
                )
            })
            .unwrap_or_default()
    }

    pub fn cell_search_text(&self, row: usize, col: usize) -> String {
        self.rows
            .get(row)
            .and_then(|row_data| row_data.get(col))
            .map(|cell| {
                let format = self.cell_format_at(row, col);
                format_cell_search(
                    cell,
                    format.and_then(|value| value.number_format).as_deref(),
                )
            })
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DocumentData {
    pub path: String,
    pub file_name: String,
    pub sheets: Vec<DocumentSheet>,
}

fn excel_cell_key(row_index: usize, col_index: usize) -> String {
    let mut col = col_index + 1;
    let mut letters = String::new();
    while col > 0 {
        let rem = (col - 1) % 26;
        letters.insert(0, (b'A' + rem as u8) as char);
        col = (col - 1) / 26;
    }
    format!("{letters}{}", row_index + 1)
}

fn rich_projection_extent(rich: &RichMetadata) -> SheetExtent {
    let mut extent = SheetExtent::default();
    for key in rich
        .cell_formats
        .keys()
        .chain(rich.cell_styles.keys())
        .chain(rich.hyperlinks.keys())
    {
        if let Some((row, col)) = parse_cell_address(key) {
            extent.row_count = extent.row_count.max(row + 1);
            extent.column_count = extent.column_count.max(col + 1);
        }
    }
    for row in &rich.hidden_rows {
        extent.row_count = extent.row_count.max(row + 1);
    }
    for col in &rich.hidden_columns {
        extent.column_count = extent.column_count.max(col + 1);
    }
    for drawing in &rich.drawings {
        extent.row_count = extent.row_count.max(
            drawing
                .to_row
                .unwrap_or(drawing.from_row)
                .max(drawing.from_row) as usize
                + 1,
        );
        extent.column_count = extent.column_count.max(
            drawing
                .to_col
                .unwrap_or(drawing.from_col)
                .max(drawing.from_col) as usize
                + 1,
        );
    }
    for image in &rich.images {
        let (from, to) = match &image.anchor {
            ImageAnchor::OneCell { from, .. } => (from, None),
            ImageAnchor::TwoCell { from, to } => (from, Some(to)),
        };
        extent.row_count = extent
            .row_count
            .max(to.map_or(from.row, |to| to.row).max(from.row) as usize + 1);
        extent.column_count = extent
            .column_count
            .max(to.map_or(from.col, |to| to.col).max(from.col) as usize + 1);
    }
    extent
}

fn parse_cell_address(key: &str) -> Option<(usize, usize)> {
    let mut col = 0usize;
    let mut row = 0usize;
    let mut saw_digit = false;
    for byte in key.bytes() {
        if byte.is_ascii_alphabetic() && !saw_digit {
            col = col
                .checked_mul(26)?
                .checked_add(usize::from(byte.to_ascii_uppercase() - b'A' + 1))?;
        } else if byte.is_ascii_digit() {
            saw_digit = true;
            row = row.checked_mul(10)?.checked_add(usize::from(byte - b'0'))?;
        } else {
            return None;
        }
    }
    (col > 0 && row > 0).then_some((row - 1, col - 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::CellNumber;

    #[test]
    fn search_snapshot_retains_display_metadata_but_drops_unrelated_rich_state() {
        let sheet = DocumentSheet {
            name: "Sheet1".to_string(),
            rows: vec![vec![CellValue::Number(CellNumber::from_f64(0.4).unwrap())]],
            merges: vec![MergeRange {
                start_row: 0,
                start_col: 0,
                end_row: 1,
                end_col: 1,
            }],
            column_widths: Some(HashMap::from([(0, 120)])),
            rich: RichMetadata {
                cell_formats: HashMap::from([(
                    "A1".to_string(),
                    CellFormat {
                        number_format: Some("0%".to_string()),
                        style_id: None,
                    },
                )]),
                hyperlinks: HashMap::from([(
                    "A1".to_string(),
                    Hyperlink {
                        url: "https://example.com".to_string(),
                        tooltip: None,
                        location: false,
                    },
                )]),
                ..Default::default()
            },
            ..Default::default()
        };

        let snapshot = sheet.search_snapshot();

        assert_eq!(snapshot.cell_display_text(0, 0), "40%");
        assert!(snapshot.merges.is_empty());
        assert!(snapshot.column_widths.is_none());
        assert!(snapshot.rich.hyperlinks.is_empty());
        assert!(snapshot.rich.cell_formats.contains_key("A1"));
    }
}
