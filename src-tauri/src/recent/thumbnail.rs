use std::io::Cursor;

use base64::Engine;
use image::{ImageBuffer, Rgba};

use crate::types::{CellValue, FileData};

const THUMBNAIL_WIDTH: u32 = 200;
const CELL_WIDTH: u32 = 40;
const CELL_HEIGHT: u32 = 20;
const MAX_ROWS: usize = 10;
const MAX_COLS: usize = 10;

#[derive(Clone, Debug)]
enum ThumbnailCell {
    Empty,
    Number,
    String { is_link: bool },
    Bool,
    Error,
}

pub(crate) struct ThumbnailSnapshot {
    rows: Vec<Vec<ThumbnailCell>>,
}

pub(crate) fn capture_thumbnail(file_data: &FileData) -> Option<ThumbnailSnapshot> {
    thumbnail_rows_from_file_data(file_data).map(|rows| ThumbnailSnapshot { rows })
}

pub(crate) fn generate_thumbnail(snapshot: ThumbnailSnapshot) -> Option<String> {
    let rows = snapshot.rows;
    if rows.is_empty() {
        return None;
    }

    let num_cols = rows[0].len() as u32;
    let num_rows = rows.len() as u32;

    let width = THUMBNAIL_WIDTH.max(num_cols * CELL_WIDTH);
    let height = num_rows * CELL_HEIGHT;

    let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(width, height);

    for pixel in img.pixels_mut() {
        *pixel = Rgba([255, 255, 255, 255]);
    }

    for (row_idx, row) in rows.iter().enumerate() {
        for (col_idx, cell) in row.iter().enumerate() {
            let x = (col_idx as u32) * CELL_WIDTH;
            let y = (row_idx as u32) * CELL_HEIGHT;

            let bg_color = get_cell_color(cell);
            fill_rect(
                &mut img,
                x + 1,
                y + 1,
                CELL_WIDTH - 2,
                CELL_HEIGHT - 2,
                bg_color,
            );
        }
    }

    let mut buffer = Cursor::new(Vec::new());
    img.write_to(&mut buffer, image::ImageFormat::Png).ok()?;

    let base64_str = base64::engine::general_purpose::STANDARD.encode(buffer.into_inner());
    Some(format!("data:image/png;base64,{}", base64_str))
}

fn thumbnail_rows_from_file_data(file_data: &FileData) -> Option<Vec<Vec<ThumbnailCell>>> {
    let sheet = file_data.sheets.first()?;
    let row_count = sheet.rows.len().min(MAX_ROWS);
    let col_count = sheet
        .rows
        .iter()
        .take(row_count)
        .map(|row| row.len().min(MAX_COLS))
        .max()
        .unwrap_or(0);
    if row_count == 0 || col_count == 0 {
        return None;
    }

    let mut rows = vec![vec![ThumbnailCell::Empty; col_count]; row_count];
    for (row_idx, row) in sheet.rows.iter().take(row_count).enumerate() {
        for (col_idx, cell) in row.iter().take(col_count).enumerate() {
            rows[row_idx][col_idx] = thumbnail_cell_from_value(cell);
        }
    }

    Some(rows)
}

fn thumbnail_cell_from_value(cell: &CellValue) -> ThumbnailCell {
    match cell {
        CellValue::Null => ThumbnailCell::Empty,
        CellValue::String(value) if value.is_empty() => ThumbnailCell::Empty,
        CellValue::String(value) => ThumbnailCell::String {
            is_link: value.starts_with("http") || value.starts_with("www"),
        },
        CellValue::Number(_) => ThumbnailCell::Number,
        CellValue::Boolean(_) => ThumbnailCell::Bool,
        CellValue::Formula {
            cached_value,
            error,
            ..
        } => {
            if error.is_some() {
                ThumbnailCell::Error
            } else {
                thumbnail_cell_from_value(cached_value)
            }
        }
    }
}

fn fill_rect(
    img: &mut ImageBuffer<Rgba<u8>, Vec<u8>>,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    color: Rgba<u8>,
) {
    for j in y..(y + h).min(img.height()) {
        for i in x..(x + w).min(img.width()) {
            img.put_pixel(i, j, color);
        }
    }
}

fn get_cell_color(cell: &ThumbnailCell) -> Rgba<u8> {
    match cell {
        ThumbnailCell::Empty => Rgba([245, 245, 245, 255]),
        ThumbnailCell::Number => Rgba([230, 242, 255, 255]),
        ThumbnailCell::String { is_link } => {
            if *is_link {
                Rgba([255, 240, 230, 255])
            } else {
                Rgba([240, 255, 240, 255])
            }
        }
        ThumbnailCell::Bool => Rgba([255, 250, 230, 255]),
        ThumbnailCell::Error => Rgba([255, 230, 230, 255]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ReadOnlyRichProjection, SheetData};

    #[test]
    fn generates_thumbnail_from_file_projection_without_workbook_bytes() {
        let file_data = FileData {
            path: String::new(),
            file_name: "projection.xlsx".to_string(),
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows: vec![vec![
                    CellValue::String("text".to_string()),
                    CellValue::Number(serde_json::Value::from(42)),
                    CellValue::Boolean(true),
                    CellValue::Formula {
                        formula: "=A1".to_string(),
                        cached_value: Box::new(CellValue::String("cached".to_string())),
                        error: None,
                    },
                ]],
                merges: Vec::new(),
                rich: ReadOnlyRichProjection::default(),
                ..Default::default()
            }],
        };

        let snapshot = capture_thumbnail(&file_data).expect("thumbnail snapshot");
        let thumbnail = generate_thumbnail(snapshot).expect("thumbnail");

        assert!(thumbnail.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn thumbnail_snapshot_does_not_retain_cell_text() {
        let file_data = FileData {
            path: String::new(),
            file_name: "projection.xlsx".to_string(),
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows: vec![vec![CellValue::String("https://example.com".repeat(1024))]],
                ..Default::default()
            }],
        };

        let snapshot = capture_thumbnail(&file_data).expect("thumbnail snapshot");

        assert!(matches!(
            snapshot.rows[0][0],
            ThumbnailCell::String { is_link: true }
        ));
    }
}
