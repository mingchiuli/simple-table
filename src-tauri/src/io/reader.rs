use calamine::{open_workbook, Reader, Xlsx, Xls, Ods, Data};

use crate::error::AppError;
use crate::types::{CellValue, FileData, MergeRange, SheetData, SheetIndex};
use csv::ReaderBuilder;
use serde_json::Value;
use std::path::Path;



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

fn read_excel(path: &Path) -> Result<FileData, AppError> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .ok_or(AppError::UnsupportedFormat)?;

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let sheets: Vec<SheetData> = match extension.as_str() {
        "xlsx" => read_xlsx(path)?,
        "xls" => read_xls(path)?,
        "ods" => read_ods(path)?,
        _ => return Err(AppError::UnsupportedFormat),
    };

    Ok(FileData { file_name, sheets })
}

fn read_xlsx(path: &Path) -> Result<Vec<SheetData>, AppError> {
    let mut workbook: Xlsx<std::io::BufReader<std::fs::File>> =
        open_workbook(path).map_err(|e: calamine::XlsxError| AppError::ReadError(e.to_string()))?;

    // Load merged regions first
    workbook
        .load_merged_regions()
        .map_err(|e| AppError::ReadError(e.to_string()))?;

    let sheet_names = workbook.sheet_names().to_vec();

    // Collect merged regions data to avoid borrowing issues
    let merged_data: Vec<(String, u32, u16, u32, u16)> = workbook
        .merged_regions()
        .iter()
        .flat_map(|(name, _, dims)| {
            std::iter::once((
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
            .map(|row| {
                row.iter()
                    .map(|cell| cell_to_value(cell.clone()))
                    .collect()
            })
            .collect();

        // Read merged cells for this sheet
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

    Ok(sheets)
}

fn read_xls(path: &Path) -> Result<Vec<SheetData>, AppError> {
    let mut workbook: Xls<std::io::BufReader<std::fs::File>> =
        open_workbook(path).map_err(|e: calamine::XlsError| AppError::ReadError(e.to_string()))?;
    let sheet_names = workbook.sheet_names().to_vec();
    Ok(sheet_names
        .iter()
        .filter_map(|sheet_name| {
            let range = workbook.worksheet_range(sheet_name).ok()?;
            let rows: Vec<Vec<CellValue>> = range
                .rows()
                .map(|row| {
                    row.iter()
                        .map(|cell| cell_to_value(cell.clone()))
                        .collect()
                })
                .collect();

            // Read merged cells (Xlsx only, other formats not supported)
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
        .collect())
}

fn read_ods(path: &Path) -> Result<Vec<SheetData>, AppError> {
    let mut workbook: Ods<std::io::BufReader<std::fs::File>> =
        open_workbook(path).map_err(|e: calamine::OdsError| AppError::ReadError(e.to_string()))?;
    let sheet_names = workbook.sheet_names().to_vec();
    Ok(sheet_names
        .iter()
        .filter_map(|sheet_name| {
            let range = workbook.worksheet_range(sheet_name).ok()?;
            let rows: Vec<Vec<CellValue>> = range
                .rows()
                .map(|row| {
                    row.iter()
                        .map(|cell| cell_to_value(cell.clone()))
                        .collect()
                })
                .collect();

            // Read merged cells (Xlsx only, other formats not supported)
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
        .collect())
}

fn read_csv(path: &Path) -> Result<FileData, AppError> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let mut reader = ReaderBuilder::new()
        .has_headers(false)
        .from_path(path)
        .map_err(|e| AppError::ReadError(e.to_string()))?;

    let mut rows: Vec<Vec<CellValue>> = Vec::new();

    for result in reader.records() {
        let record = result.map_err(|e| AppError::ReadError(e.to_string()))?;
        let row: Vec<CellValue> = record
            .iter()
            .map(|field| {
                if field.is_empty() {
                    CellValue::Null
                } else if let Ok(int_val) = field.parse::<i64>() {
                    // 超出 JS 安全范围的整数保持为字符串
                    if int_val > 9007199254740991 || int_val < -9007199254740991 {
                        return CellValue::String(field.to_string());
                    }
                    // 使用 serde_json::Value 精确存储整数
                    CellValue::Number(Value::from(int_val))
                } else if let Ok(num) = field.parse::<f64>() {
                    // 尝试解析为浮点数
                    CellValue::Number(Value::from(num))
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

pub fn read_file(path: &Path) -> Result<FileData, AppError> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .ok_or(AppError::UnsupportedFormat)?;

    match extension.as_str() {
        "xlsx" | "xls" | "ods" => read_excel(path),
        "csv" => read_csv(path),
        _ => Err(AppError::UnsupportedFormat),
    }
}
