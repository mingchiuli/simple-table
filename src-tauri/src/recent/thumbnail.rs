use std::io::Cursor;

use base64::Engine;
use csv::ReaderBuilder;
use image::{ImageBuffer, Rgba};
use umya_spreadsheet::{Cell, reader};

const THUMBNAIL_WIDTH: u32 = 200;
const CELL_WIDTH: u32 = 40;
const CELL_HEIGHT: u32 = 20;
const MAX_ROWS: usize = 10;
const MAX_COLS: usize = 10;

#[derive(Clone, Debug)]
enum ThumbnailCell {
    Empty,
    Number,
    String(String),
    Bool,
    Error,
}

fn get_format_from_extension(ext: &str) -> Option<&'static str> {
    match ext.to_lowercase().as_str() {
        "xlsx" | "xlsm" => Some("xlsx"),
        "csv" => Some("csv"),
        _ => None,
    }
}

pub fn generate_thumbnail_from_bytes(bytes: &[u8], extension: &str) -> Option<String> {
    let format = get_format_from_extension(extension)?;

    let rows = match format {
        "xlsx" => read_xlsx_from_bytes(bytes)?,
        "csv" => read_csv_from_bytes(bytes)?,
        _ => return None,
    };

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

fn read_xlsx_from_bytes(bytes: &[u8]) -> Option<Vec<Vec<ThumbnailCell>>> {
    let workbook = reader::xlsx::read_reader(Cursor::new(bytes.to_vec()), true).ok()?;
    let worksheet = workbook.sheet(0).ok()?;
    let (highest_col, highest_row) = worksheet.highest_column_and_row();
    let row_count = (highest_row as usize).min(MAX_ROWS);
    let col_count = (highest_col as usize).min(MAX_COLS);
    if row_count == 0 || col_count == 0 {
        return None;
    }

    let mut rows = vec![vec![ThumbnailCell::Empty; col_count]; row_count];
    for cell in worksheet.cells() {
        let row_idx = cell.coordinate().row_num().saturating_sub(1) as usize;
        let col_idx = cell.coordinate().col_num().saturating_sub(1) as usize;
        if row_idx < row_count && col_idx < col_count {
            rows[row_idx][col_idx] = thumbnail_cell_from_umya(cell);
        }
    }

    Some(rows)
}

fn thumbnail_cell_from_umya(cell: &Cell) -> ThumbnailCell {
    match cell.data_type() {
        "b" => ThumbnailCell::Bool,
        "e" => ThumbnailCell::Error,
        "n" => ThumbnailCell::Number,
        _ => {
            let value = cell.value().into_owned();
            if value.is_empty() {
                ThumbnailCell::Empty
            } else {
                ThumbnailCell::String(value)
            }
        }
    }
}

fn read_csv_from_bytes(bytes: &[u8]) -> Option<Vec<Vec<ThumbnailCell>>> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut reader = ReaderBuilder::new().has_headers(false).from_reader(cursor);

    let mut rows = Vec::new();
    for result in reader.records() {
        let record = result.ok()?;
        let row: Vec<ThumbnailCell> = record
            .iter()
            .take(MAX_COLS)
            .map(|field| {
                if field.is_empty() {
                    ThumbnailCell::Empty
                } else if field.parse::<i64>().is_ok() || field.parse::<f64>().is_ok() {
                    ThumbnailCell::Number
                } else if field.eq_ignore_ascii_case("true") || field.eq_ignore_ascii_case("false")
                {
                    ThumbnailCell::Bool
                } else {
                    ThumbnailCell::String(field.to_string())
                }
            })
            .collect();
        rows.push(row);
        if rows.len() >= MAX_ROWS {
            break;
        }
    }

    Some(rows)
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
        ThumbnailCell::String(s) => {
            if s.starts_with("http") || s.starts_with("www") {
                Rgba([255, 240, 230, 255])
            } else {
                Rgba([240, 255, 240, 255])
            }
        }
        ThumbnailCell::Bool => Rgba([255, 250, 230, 255]),
        ThumbnailCell::Error => Rgba([255, 230, 230, 255]),
    }
}
