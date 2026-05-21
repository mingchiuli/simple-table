use std::io::{BufReader, Cursor};

use base64::Engine;
use calamine::{Data, Ods, Reader, Xls, Xlsx};
use csv::ReaderBuilder;
use image::{ImageBuffer, Rgba};

const THUMBNAIL_WIDTH: u32 = 200;
const CELL_WIDTH: u32 = 40;
const CELL_HEIGHT: u32 = 20;
const MAX_ROWS: usize = 10;
const MAX_COLS: usize = 10;

fn get_format_from_extension(ext: &str) -> Option<&'static str> {
    match ext.to_lowercase().as_str() {
        "xlsx" => Some("xlsx"),
        "xls" => Some("xls"),
        "ods" => Some("ods"),
        "csv" => Some("csv"),
        _ => None,
    }
}

pub fn generate_thumbnail_from_bytes(bytes: &[u8], extension: &str) -> Option<String> {
    let format = get_format_from_extension(extension)?;

    let rows = match format {
        "xlsx" => read_xlsx_from_bytes(bytes)?,
        "xls" => read_xls_from_bytes(bytes)?,
        "ods" => read_ods_from_bytes(bytes)?,
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

fn read_xlsx_from_bytes(bytes: &[u8]) -> Option<Vec<Vec<Data>>> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut workbook: Xlsx<BufReader<Cursor<Vec<u8>>>> = Xlsx::new(BufReader::new(cursor)).ok()?;
    read_sheet_data_from_bytes(&mut workbook)
}

fn read_xls_from_bytes(bytes: &[u8]) -> Option<Vec<Vec<Data>>> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut workbook: Xls<BufReader<Cursor<Vec<u8>>>> = Xls::new(BufReader::new(cursor)).ok()?;
    read_sheet_data_xls_from_bytes(&mut workbook)
}

fn read_ods_from_bytes(bytes: &[u8]) -> Option<Vec<Vec<Data>>> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut workbook: Ods<BufReader<Cursor<Vec<u8>>>> = Ods::new(BufReader::new(cursor)).ok()?;
    read_sheet_data_ods_from_bytes(&mut workbook)
}

fn read_csv_from_bytes(bytes: &[u8]) -> Option<Vec<Vec<Data>>> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut reader = ReaderBuilder::new().has_headers(false).from_reader(cursor);

    let mut rows = Vec::new();
    for result in reader.records() {
        let record = result.ok()?;
        let row: Vec<Data> = record
            .iter()
            .map(|field| {
                if field.is_empty() {
                    Data::Empty
                } else if let Ok(int_val) = field.parse::<i64>() {
                    Data::Int(int_val)
                } else if let Ok(float_val) = field.parse::<f64>() {
                    Data::Float(float_val)
                } else if field.to_lowercase() == "true" {
                    Data::Bool(true)
                } else if field.to_lowercase() == "false" {
                    Data::Bool(false)
                } else {
                    Data::String(field.to_string())
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

fn read_sheet_data_from_bytes(
    workbook: &mut Xlsx<BufReader<Cursor<Vec<u8>>>>,
) -> Option<Vec<Vec<Data>>> {
    let sheets = workbook.sheet_names().to_owned();
    if sheets.is_empty() {
        return None;
    }

    let sheet_name = &sheets[0];
    let range = workbook.worksheet_range(sheet_name).ok()?;

    Some(
        range
            .rows()
            .take(MAX_ROWS)
            .map(|row| row.iter().take(MAX_COLS).cloned().collect())
            .collect(),
    )
}

fn read_sheet_data_xls_from_bytes(
    workbook: &mut Xls<BufReader<Cursor<Vec<u8>>>>,
) -> Option<Vec<Vec<Data>>> {
    let sheets = workbook.sheet_names().to_owned();
    if sheets.is_empty() {
        return None;
    }

    let sheet_name = &sheets[0];
    let range = workbook.worksheet_range(sheet_name).ok()?;

    Some(
        range
            .rows()
            .take(MAX_ROWS)
            .map(|row| row.iter().take(MAX_COLS).cloned().collect())
            .collect(),
    )
}

fn read_sheet_data_ods_from_bytes(
    workbook: &mut Ods<BufReader<Cursor<Vec<u8>>>>,
) -> Option<Vec<Vec<Data>>> {
    let sheets = workbook.sheet_names().to_owned();
    if sheets.is_empty() {
        return None;
    }

    let sheet_name = &sheets[0];
    let range = workbook.worksheet_range(sheet_name).ok()?;

    Some(
        range
            .rows()
            .take(MAX_ROWS)
            .map(|row| row.iter().take(MAX_COLS).cloned().collect())
            .collect(),
    )
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

fn get_cell_color(cell: &Data) -> Rgba<u8> {
    match cell {
        Data::Empty => Rgba([245, 245, 245, 255]),
        Data::Int(_) => Rgba([230, 242, 255, 255]),
        Data::Float(_) => Rgba([230, 242, 255, 255]),
        Data::String(s) => {
            if s.starts_with("http") || s.starts_with("www") {
                Rgba([255, 240, 230, 255])
            } else {
                Rgba([240, 255, 240, 255])
            }
        }
        Data::Bool(_) => Rgba([255, 250, 230, 255]),
        Data::DateTime(_) => Rgba([240, 240, 255, 255]),
        Data::DateTimeIso(_) => Rgba([240, 240, 255, 255]),
        Data::DurationIso(_) => Rgba([240, 240, 255, 255]),
        Data::Error(_) => Rgba([255, 230, 230, 255]),
    }
}
