use calamine::{open_workbook, Reader, Xlsx, Xls, Ods, Data};

use crate::error::AppError;
use crate::types::{CellValue, FileData, SheetData, SheetIndex};
use csv::ReaderBuilder;
use std::path::Path;



fn cell_to_value(cell: Data) -> CellValue {
    match cell {
        Data::String(s) => CellValue::String(s),
        Data::Float(f) => CellValue::Number(f),
        Data::Int(i) => CellValue::Number(i as f64),
        Data::Bool(b) => CellValue::Boolean(b),
        Data::DateTime(dt) => CellValue::Number(dt.as_f64()),
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
            let index = SheetIndex::default();
            Some(SheetData {
                name: sheet_name.clone(),
                rows,
                index,
            })
        })
        .collect())
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
            let index = SheetIndex::default();
            Some(SheetData {
                name: sheet_name.clone(),
                rows,
                index,
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
            let index = SheetIndex::default();
            Some(SheetData {
                name: sheet_name.clone(),
                rows,
                index,
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
        .has_headers(true)
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
                } else if let Ok(num) = field.parse::<f64>() {
                    CellValue::Number(num)
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
            index,
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
