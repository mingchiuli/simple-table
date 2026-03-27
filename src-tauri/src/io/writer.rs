use crate::error::AppError;
use crate::types::{CellValue, FileData};
use rust_xlsxwriter::*;

/// 生成文件字节（用于 Android content:// URI 场景）
pub fn generate_file_bytes(file_data: &FileData) -> Result<(String, Vec<u8>), AppError> {
    // 确定文件扩展名
    let file_name = &file_data.file_name;
    let extension = std::path::Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_else(|| "xlsx".to_string());

    match extension.as_str() {
        "xlsx" => {
            let bytes = write_excel_to_bytes(file_data)?;
            Ok((format!("{}.xlsx", file_name.replace(".xlsx", "").replace(".csv", "")), bytes))
        }
        "csv" => {
            let bytes = write_csv_to_bytes(file_data)?;
            Ok((format!("{}.csv", file_name.replace(".xlsx", "").replace(".csv", "")), bytes))
        }
        _ => Err(AppError::UnsupportedFormat),
    }
}

fn write_excel_to_bytes(file_data: &FileData) -> Result<Vec<u8>, AppError> {
    let mut workbook = Workbook::new();

    for sheet in &file_data.sheets {
        let worksheet = workbook.add_worksheet();
        worksheet
            .set_name(&sheet.name)
            .map_err(|e| AppError::WriteError(e.to_string()))?;

        for (row_idx, row) in sheet.rows.iter().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
                let row_u32 = row_idx as u32;
                let col_u16 = col_idx as u16;
                match cell {
                    CellValue::String(s) => {
                        worksheet
                            .write(row_u32, col_u16, s.as_str())
                            .map_err(|e| AppError::WriteError(e.to_string()))?;
                    }
                    CellValue::Number(n) => {
                        if let Some(num) = n.as_f64() {
                            worksheet
                                .write(row_u32, col_u16, num)
                                .map_err(|e| AppError::WriteError(e.to_string()))?;
                        } else if let Some(num) = n.as_i64() {
                            const F64_SAFE_MAX: i64 = 9_007_199_254_740_991;
                            const F64_SAFE_MIN: i64 = -9_007_199_254_740_991;
                            if num >= F64_SAFE_MIN && num <= F64_SAFE_MAX {
                                worksheet
                                    .write(row_u32, col_u16, num as f64)
                                    .map_err(|e| AppError::WriteError(e.to_string()))?;
                            } else {
                                worksheet
                                    .write(row_u32, col_u16, &num.to_string())
                                    .map_err(|e| AppError::WriteError(e.to_string()))?;
                            }
                        } else {
                            worksheet
                                .write(row_u32, col_u16, &n.to_string())
                                .map_err(|e| AppError::WriteError(e.to_string()))?;
                        }
                    }
                    CellValue::Boolean(b) => {
                        worksheet
                            .write(row_u32, col_u16, *b)
                            .map_err(|e| AppError::WriteError(e.to_string()))?;
                    }
                    CellValue::Null => {
                        worksheet
                            .write_blank(row_u32, col_u16, &Format::new())
                            .map_err(|e| AppError::WriteError(e.to_string()))?;
                    }
                }
            }
        }

        for merge in &sheet.merges {
            let value = sheet
                .rows
                .get(merge.start_row as usize)
                .and_then(|r| r.get(merge.start_col as usize))
                .cloned()
                .unwrap_or(CellValue::Null);

            let s = match value {
                CellValue::String(s) => s.clone(),
                CellValue::Number(n) => n.to_string(),
                CellValue::Boolean(b) => b.to_string(),
                CellValue::Null => String::new(),
            };

            worksheet
                .merge_range(
                    merge.start_row,
                    merge.start_col,
                    merge.end_row,
                    merge.end_col,
                    &s,
                    &Format::new(),
                )
                .map_err(|e| AppError::WriteError(e.to_string()))?;
        }
    }

    let bytes = workbook
        .save_to_buffer()
        .map_err(|e| AppError::WriteError(e.to_string()))?;
    Ok(bytes)
}

fn write_csv_to_bytes(file_data: &FileData) -> Result<Vec<u8>, AppError> {
    let mut buffer = Vec::new();
    {
        let mut writer = csv::Writer::from_writer(&mut buffer);

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
            .map_err(|e: std::io::Error| AppError::WriteError(e.to_string()))?;
    }
    Ok(buffer)
}
