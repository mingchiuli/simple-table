use crate::error::AppError;
use crate::types::{CellValue, FileData};
use std::path::Path;
use xlsxwriter::*;

fn write_excel(path: &Path, file_data: &FileData) -> Result<(), AppError> {
    let path_str = path
        .to_str()
        .ok_or(AppError::WriteError("Invalid path".to_string()))?;
    let workbook =
        Workbook::new(path_str).map_err(|e| AppError::WriteError(e.to_string()))?;

    for sheet in &file_data.sheets {
        let mut worksheet = workbook
            .add_worksheet(Some(&sheet.name))
            .map_err(|e| AppError::WriteError(e.to_string()))?;

        for (row_idx, row) in sheet.rows.iter().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
                let row_u32 = row_idx as u32;
                let col_u16 = col_idx as u16;
                match cell {
                    CellValue::String(s) => {
                        worksheet
                            .write_string(row_u32, col_u16, s, None)
                            .map_err(|e| AppError::WriteError(e.to_string()))?;
                    }
                    CellValue::Number(n) => {
                        worksheet
                            .write_number(row_u32, col_u16, *n, None)
                            .map_err(|e| AppError::WriteError(e.to_string()))?;
                    }
                    CellValue::Boolean(b) => {
                        worksheet
                            .write_boolean(row_u32, col_u16, *b, None)
                            .map_err(|e| AppError::WriteError(e.to_string()))?;
                    }
                    CellValue::Null => {
                        worksheet
                            .write_blank(row_u32, col_u16, None)
                            .map_err(|e| AppError::WriteError(e.to_string()))?;
                    }
                }
            }
        }
    }

    workbook
        .close()
        .map_err(|e| AppError::WriteError(e.to_string()))?;
    Ok(())
}

fn write_csv(path: &Path, file_data: &FileData) -> Result<(), AppError> {
    let mut writer =
        csv::Writer::from_path(path).map_err(|e| AppError::WriteError(e.to_string()))?;

    if let Some(first_sheet) = file_data.sheets.first() {
        for row in &first_sheet.rows {
            let string_row: Vec<String> = row
                .iter()
                .map(|cell| match cell {
                    CellValue::String(s) => s.clone(),
                    CellValue::Number(n) => n.to_string(),
                    CellValue::Boolean(b) => b.to_string(),
                    CellValue::Null => String::new(),
                })
                .collect();
            writer
                .write_record(&string_row)
                .map_err(|e| AppError::WriteError(e.to_string()))?;
        }
    }

    writer
        .flush()
        .map_err(|e| AppError::WriteError(e.to_string()))?;
    Ok(())
}

pub fn save_file(path: &Path, file_data: &FileData) -> Result<(), AppError> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .ok_or(AppError::UnsupportedFormat)?;

    match extension.as_str() {
        "xlsx" => write_excel(path, file_data),
        "csv" => write_csv(path, file_data),
        _ => Err(AppError::UnsupportedFormat),
    }
}
