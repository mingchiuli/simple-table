use std::path::Path;

use crate::error::AppError;
use crate::types::{CellValue, FileData};
use rust_xlsxwriter::*;

/// 根据 FileData 生成可写入文件系统或导出的文件字节
pub fn generate_file_bytes(file_data: &FileData) -> Result<(String, Vec<u8>), AppError> {
    generate_file_bytes_for_name(file_data, &file_data.file_name)
}

/// 根据目标文件名/路径生成对应格式的字节。
///
/// 读入 .xls/.ods 后仍允许编辑，但当前写出层只支持 xlsx/csv；
/// 调用方必须传入用户最终选择的目标路径，避免用旧 file_name 推断格式。
pub fn generate_file_bytes_for_target(
    file_data: &FileData,
    target_path_or_name: &str,
) -> Result<(String, Vec<u8>), AppError> {
    let target_name = Path::new(target_path_or_name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(target_path_or_name);
    generate_file_bytes_for_name(file_data, target_name)
}

fn generate_file_bytes_for_name(
    file_data: &FileData,
    output_name: &str,
) -> Result<(String, Vec<u8>), AppError> {
    // 确定文件扩展名
    let extension = Path::new(output_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_else(|| "xlsx".to_string());
    let output_stem = Path::new(output_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("untitled");

    match extension.as_str() {
        "xlsx" => {
            let bytes = write_excel_to_bytes(file_data)?;
            Ok((format!("{output_stem}.xlsx"), bytes))
        }
        "csv" => {
            let bytes = write_csv_to_bytes(file_data)?;
            Ok((format!("{output_stem}.csv"), bytes))
        }
        _ => Err(AppError::UnsupportedFormat),
    }
}

fn write_excel_to_bytes(file_data: &FileData) -> Result<Vec<u8>, AppError> {
    let mut workbook = Workbook::new();
    let blank_format = Format::new();

    for sheet in &file_data.sheets {
        let worksheet = workbook.add_worksheet();
        worksheet
            .set_name(&sheet.name)
            .map_err(|e| AppError::WriteError(e.to_string()))?;

        for (row_idx, row) in sheet.rows.iter().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
                write_cell(
                    worksheet,
                    row_idx as u32,
                    col_idx as u16,
                    cell,
                    &blank_format,
                )?;
            }
        }

        for merge in &sheet.merges {
            worksheet
                .merge_range(
                    merge.start_row,
                    merge.start_col,
                    merge.end_row,
                    merge.end_col,
                    "",
                    &blank_format,
                )
                .map_err(|e| AppError::WriteError(e.to_string()))?;

            let value = sheet
                .rows
                .get(merge.start_row as usize)
                .and_then(|r| r.get(merge.start_col as usize))
                .unwrap_or(&CellValue::Null);
            write_cell(
                worksheet,
                merge.start_row,
                merge.start_col,
                value,
                &blank_format,
            )?;
        }
    }

    let bytes = workbook
        .save_to_buffer()
        .map_err(|e| AppError::WriteError(e.to_string()))?;
    Ok(bytes)
}

fn write_cell(
    worksheet: &mut Worksheet,
    row: u32,
    col: u16,
    cell: &CellValue,
    blank_format: &Format,
) -> Result<(), AppError> {
    match cell {
        CellValue::Formula {
            formula,
            cached_value,
            error,
        } => {
            let result = error
                .clone()
                .unwrap_or_else(|| cached_value.to_display_string());
            worksheet
                .write_formula(row, col, Formula::new(formula).set_result(result))
                .map_err(|e| AppError::WriteError(e.to_string()))?;
        }
        CellValue::String(s) => {
            worksheet
                .write(row, col, s.as_str())
                .map_err(|e| AppError::WriteError(e.to_string()))?;
        }
        CellValue::Number(n) => {
            if let Some(num) = n.as_i64() {
                const F64_SAFE_MAX: i64 = 9_007_199_254_740_991;
                const F64_SAFE_MIN: i64 = -9_007_199_254_740_991;
                if (F64_SAFE_MIN..=F64_SAFE_MAX).contains(&num) {
                    worksheet
                        .write(row, col, num as f64)
                        .map_err(|e| AppError::WriteError(e.to_string()))?;
                } else {
                    worksheet
                        .write(row, col, num.to_string())
                        .map_err(|e| AppError::WriteError(e.to_string()))?;
                }
            } else if let Some(num) = n.as_f64() {
                if num.is_finite() {
                    worksheet
                        .write(row, col, num)
                        .map_err(|e| AppError::WriteError(e.to_string()))?;
                } else {
                    // NaN/Infinity 写为空白单元格
                    worksheet
                        .write_blank(row, col, blank_format)
                        .map_err(|e| AppError::WriteError(e.to_string()))?;
                }
            } else {
                worksheet
                    .write(row, col, n.to_string())
                    .map_err(|e| AppError::WriteError(e.to_string()))?;
            }
        }
        CellValue::Boolean(b) => {
            worksheet
                .write(row, col, *b)
                .map_err(|e| AppError::WriteError(e.to_string()))?;
        }
        CellValue::Null => {
            worksheet
                .write_blank(row, col, blank_format)
                .map_err(|e| AppError::WriteError(e.to_string()))?;
        }
    }

    Ok(())
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
                        CellValue::Formula { .. } => cell.to_display_string(),
                        CellValue::String(s) => s.clone(),
                        CellValue::Number(n) => {
                            // f64 的 NaN/Infinity 不可机读，写空字符串
                            if let Some(f) = n.as_f64()
                                && !f.is_finite()
                                && n.as_i64().is_none()
                            {
                                return String::new();
                            }
                            n.to_string()
                        }
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

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::io::codec::reader::read_file_from_bytes;
    use crate::types::{MergeRange, SheetData};

    #[test]
    fn preserves_formula_in_merged_top_left_cell() {
        let file_data = FileData {
            path: String::new(),
            file_name: "merged-formula.xlsx".to_string(),
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows: vec![vec![
                    CellValue::Formula {
                        formula: "=1+2".to_string(),
                        cached_value: Box::new(CellValue::Number(Value::from(3))),
                        error: None,
                    },
                    CellValue::Null,
                ]],
                merges: vec![MergeRange {
                    start_row: 0,
                    start_col: 0,
                    end_row: 0,
                    end_col: 1,
                }],
                ..Default::default()
            }],
        };

        let (_, bytes) =
            generate_file_bytes_for_target(&file_data, "merged-formula.xlsx").expect("write xlsx");
        let read_back = read_file_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "merged-formula.xlsx".to_string(),
        )
        .expect("read xlsx");

        match &read_back.sheets[0].rows[0][0] {
            CellValue::Formula { formula, .. } => assert_eq!(formula, "=1+2"),
            value => panic!("expected formula, got {value:?}"),
        }
    }
}
