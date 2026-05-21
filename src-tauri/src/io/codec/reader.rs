use std::io::{BufReader, Cursor};
use std::iter;

use calamine::{Data, Ods, Reader, Xls, Xlsx};

use crate::error::AppError;
use crate::types::{CellValue, FileData, MergeRange, SheetData, SheetIndex};
use csv::ReaderBuilder;
use serde_json::Value;

fn cell_to_value(cell: Data) -> CellValue {
    match cell {
        Data::String(s) => CellValue::String(s),
        // 使用 serde_json::Value 来精确存储整数
        Data::Float(f) => CellValue::Number(Value::from(f)),
        Data::Int(i) => CellValue::Number(Value::from(i)),
        Data::Bool(b) => CellValue::Boolean(b),
        Data::DateTime(dt) => CellValue::Number(Value::from(dt.as_f64())),
        Data::DateTimeIso(s) => CellValue::String(s),
        Data::DurationIso(s) => CellValue::String(s),
        Data::Error(e) => CellValue::String(format!("{:?}", e)),
        Data::Empty => CellValue::Null,
    }
}

/// 从已读取的文件字节解析 FileData
pub fn read_file_from_bytes(
    extension: &str,
    bytes: Vec<u8>,
    path: String,
    file_name: String,
) -> Result<FileData, AppError> {
    let cursor = Cursor::new(bytes);

    match extension.to_lowercase().as_str() {
        "xlsx" => read_xlsx_from_bytes(cursor, path, file_name),
        "xls" => read_xls_from_bytes(cursor, path, file_name),
        "ods" => read_ods_from_bytes(cursor, path, file_name),
        "csv" => read_csv_from_bytes(cursor, path, file_name),
        _ => Err(AppError::UnsupportedFormat),
    }
}

fn read_xlsx_from_bytes(
    cursor: Cursor<Vec<u8>>,
    path: String,
    file_name: String,
) -> Result<FileData, AppError> {
    let mut workbook: Xlsx<BufReader<Cursor<Vec<u8>>>> = Xlsx::new(BufReader::new(cursor))
        .map_err(|e: calamine::XlsxError| AppError::ReadError(e.to_string()))?;
    read_workbook(&mut workbook, path, file_name)
}

fn read_xls_from_bytes(
    cursor: Cursor<Vec<u8>>,
    path: String,
    file_name: String,
) -> Result<FileData, AppError> {
    let mut workbook: Xls<BufReader<Cursor<Vec<u8>>>> = Xls::new(BufReader::new(cursor))
        .map_err(|e: calamine::XlsError| AppError::ReadError(e.to_string()))?;
    read_workbook_xls(&mut workbook, path, file_name)
}

fn read_ods_from_bytes(
    cursor: Cursor<Vec<u8>>,
    path: String,
    file_name: String,
) -> Result<FileData, AppError> {
    let mut workbook: Ods<BufReader<Cursor<Vec<u8>>>> = Ods::new(BufReader::new(cursor))
        .map_err(|e: calamine::OdsError| AppError::ReadError(e.to_string()))?;
    read_workbook_ods(&mut workbook, path, file_name)
}

fn read_workbook(
    workbook: &mut Xlsx<BufReader<Cursor<Vec<u8>>>>,
    path: String,
    file_name: String,
) -> Result<FileData, AppError> {
    workbook
        .load_merged_regions()
        .map_err(|e| AppError::ReadError(e.to_string()))?;

    let sheet_names = workbook.sheet_names().to_vec();

    let merged_data: Vec<(String, u32, u16, u32, u16)> = workbook
        .merged_regions()
        .iter()
        .flat_map(|(name, _, dims)| {
            iter::once((
                name.clone(),
                dims.start.0,
                dims.start.1 as u16,
                dims.end.0,
                dims.end.1 as u16,
            ))
        })
        .collect();

    let mut sheets: Vec<SheetData> = Vec::new();

    for sheet_name in &sheet_names {
        let range = match workbook.worksheet_range(sheet_name) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let rows: Vec<Vec<CellValue>> = range
            .rows()
            .map(|row| row.iter().map(|cell| cell_to_value(cell.clone())).collect())
            .collect();

        let merges: Vec<MergeRange> = merged_data
            .iter()
            .filter(|(name, _, _, _, _)| name == sheet_name)
            .map(|(_, start_r, start_c, end_r, end_c)| MergeRange {
                start_row: *start_r,
                start_col: *start_c,
                end_row: *end_r,
                end_col: *end_c,
            })
            .collect();

        let index = SheetIndex::default();
        sheets.push(SheetData {
            name: sheet_name.clone(),
            rows,
            merges,
            index,
            ..Default::default()
        });
    }

    Ok(FileData {
        path,
        file_name,
        sheets,
    })
}

fn read_workbook_xls(
    workbook: &mut Xls<BufReader<Cursor<Vec<u8>>>>,
    path: String,
    file_name: String,
) -> Result<FileData, AppError> {
    let sheet_names = workbook.sheet_names().to_vec();
    let sheets: Vec<SheetData> = sheet_names
        .iter()
        .filter_map(|sheet_name| {
            let range = workbook.worksheet_range(sheet_name).ok()?;
            let rows: Vec<Vec<CellValue>> = range
                .rows()
                .map(|row| row.iter().map(|cell| cell_to_value(cell.clone())).collect())
                .collect();

            let merges: Vec<MergeRange> = Vec::new();
            let index = SheetIndex::default();
            Some(SheetData {
                name: sheet_name.clone(),
                rows,
                merges,
                index,
                ..Default::default()
            })
        })
        .collect();
    Ok(FileData {
        path,
        file_name,
        sheets,
    })
}

fn read_workbook_ods(
    workbook: &mut Ods<BufReader<Cursor<Vec<u8>>>>,
    path: String,
    file_name: String,
) -> Result<FileData, AppError> {
    let sheet_names = workbook.sheet_names().to_vec();
    let sheets: Vec<SheetData> = sheet_names
        .iter()
        .filter_map(|sheet_name| {
            let range = workbook.worksheet_range(sheet_name).ok()?;
            let rows: Vec<Vec<CellValue>> = range
                .rows()
                .map(|row| row.iter().map(|cell| cell_to_value(cell.clone())).collect())
                .collect();

            let merges: Vec<MergeRange> = Vec::new();
            let index = SheetIndex::default();
            Some(SheetData {
                name: sheet_name.clone(),
                rows,
                merges,
                index,
                ..Default::default()
            })
        })
        .collect();
    Ok(FileData {
        path,
        file_name,
        sheets,
    })
}

/// 判断字符串是否带有前导零（如 "007"、"-0123"），需要按字符串处理避免精度丢失
fn has_leading_zero(s: &str) -> bool {
    let bytes = s.as_bytes();
    let digits = if bytes.first() == Some(&b'-') || bytes.first() == Some(&b'+') {
        &bytes[1..]
    } else {
        bytes
    };
    digits.len() > 1 && digits[0] == b'0' && digits.iter().all(|b| b.is_ascii_digit())
}

fn read_csv_from_bytes(
    cursor: Cursor<Vec<u8>>,
    path: String,
    file_name: String,
) -> Result<FileData, AppError> {
    let mut reader = ReaderBuilder::new().has_headers(false).from_reader(cursor);

    let mut rows: Vec<Vec<CellValue>> = Vec::new();

    for result in reader.records() {
        let record = result.map_err(|e| AppError::ReadError(e.to_string()))?;
        let row: Vec<CellValue> = record
            .iter()
            .map(|field| {
                if field.is_empty() {
                    CellValue::Null
                } else if has_leading_zero(field) {
                    // 保留电话号码、邮编等以 0 开头的字符串
                    CellValue::String(field.to_string())
                } else if let Ok(int_val) = field.parse::<i64>() {
                    if int_val > 9007199254740991 || int_val < -9007199254740991 {
                        return CellValue::String(field.to_string());
                    }
                    CellValue::Number(Value::from(int_val))
                } else if let Ok(num) = field.parse::<f64>() {
                    if num.is_finite() {
                        CellValue::Number(Value::from(num))
                    } else {
                        CellValue::String(field.to_string())
                    }
                } else if field.to_lowercase() == "true" {
                    CellValue::Boolean(true)
                } else if field.to_lowercase() == "false" {
                    CellValue::Boolean(false)
                } else {
                    CellValue::String(field.to_string())
                }
            })
            .collect();
        rows.push(row);
    }

    let index = SheetIndex::default();
    Ok(FileData {
        path,
        file_name,
        sheets: vec![SheetData {
            name: "Sheet1".to_string(),
            rows,
            merges: vec![],
            index,
            ..Default::default()
        }],
    })
}
