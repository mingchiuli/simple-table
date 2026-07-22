use crate::document_data::{DocumentData, DocumentSheet};
use std::collections::HashMap;
use std::io::Cursor;

use crate::document_format::SpreadsheetFileFormat;
use crate::domain::{CellNumber, CellValue};
use crate::error::AppError;
use crate::io::input_limits::{
    MAX_XLSX_ARCHIVE_ENTRIES, MAX_XLSX_UNCOMPRESSED_BYTES, validate_input_size,
};
use crate::io::projection_mapper::ProjectionMapper;
use crate::resource_limits::{
    MAX_DENSE_CELL_SLOTS, MAX_ROWS_PER_SHEET, MAX_TOTAL_ROWS, MAX_WORKBOOK_SHEETS,
    validate_file_data, validate_position,
};
use csv::ReaderBuilder;
use umya_spreadsheet::{Workbook, reader};

pub struct ReadFileResult {
    pub file_data: DocumentData,
    pub workbook: Option<Workbook>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct InputFilePreflight {
    format: SpreadsheetFileFormat,
    estimated_parse_bytes: usize,
}

impl InputFilePreflight {
    pub(crate) fn estimated_parse_bytes(self) -> usize {
        self.estimated_parse_bytes
    }
}

const CSV_PARSE_MEMORY_MULTIPLIER: usize = 3;
const XLSX_UNCOMPRESSED_MEMORY_MULTIPLIER: usize = 3;

#[cfg(test)]
pub fn read_file_with_workbook_from_bytes(
    extension: &str,
    bytes: Vec<u8>,
    path: String,
    file_name: String,
) -> Result<ReadFileResult, AppError> {
    let preflight = preflight_input_file(extension, &bytes)?;
    read_file_with_workbook_from_preflight(preflight, bytes, path, file_name)
}

pub(crate) fn preflight_input_file(
    extension: &str,
    bytes: &[u8],
) -> Result<InputFilePreflight, AppError> {
    validate_input_size(bytes.len())?;
    let format =
        SpreadsheetFileFormat::from_extension(extension).ok_or(AppError::UnsupportedFormat)?;
    let estimated_parse_bytes = match format {
        SpreadsheetFileFormat::Xlsx => {
            estimate_xlsx_parse_bytes(bytes.len(), validate_xlsx_archive(bytes)?)
        }
        SpreadsheetFileFormat::Csv => bytes.len().saturating_mul(CSV_PARSE_MEMORY_MULTIPLIER),
    };
    Ok(InputFilePreflight {
        format,
        estimated_parse_bytes,
    })
}

pub(crate) fn read_file_with_workbook_from_preflight(
    preflight: InputFilePreflight,
    bytes: Vec<u8>,
    path: String,
    file_name: String,
) -> Result<ReadFileResult, AppError> {
    match preflight.format {
        SpreadsheetFileFormat::Xlsx => read_xlsx_from_bytes(Cursor::new(bytes), path, file_name),
        SpreadsheetFileFormat::Csv => read_csv_from_bytes(Cursor::new(bytes), path, file_name),
    }
}

fn validate_xlsx_archive(bytes: &[u8]) -> Result<u64, AppError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| AppError::ReadError(error.to_string()))?;
    if archive.len() > MAX_XLSX_ARCHIVE_ENTRIES {
        return Err(AppError::ResourceLimitExceeded(format!(
            "XLSX archive entries is {}, maximum is {}",
            archive.len(),
            MAX_XLSX_ARCHIVE_ENTRIES
        )));
    }

    let mut uncompressed_bytes = 0u64;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| AppError::ReadError(error.to_string()))?;
        uncompressed_bytes = uncompressed_bytes.saturating_add(entry.size());
        if uncompressed_bytes > MAX_XLSX_UNCOMPRESSED_BYTES {
            return Err(AppError::ResourceLimitExceeded(format!(
                "XLSX uncompressed bytes exceed the maximum of {MAX_XLSX_UNCOMPRESSED_BYTES}"
            )));
        }
    }
    Ok(uncompressed_bytes)
}

fn estimate_xlsx_parse_bytes(input_bytes: usize, uncompressed_bytes: u64) -> usize {
    let uncompressed_bytes = usize::try_from(uncompressed_bytes).unwrap_or(usize::MAX);
    input_bytes
        .saturating_add(uncompressed_bytes.saturating_mul(XLSX_UNCOMPRESSED_MEMORY_MULTIPLIER))
}

fn read_xlsx_from_bytes(
    cursor: Cursor<Vec<u8>>,
    path: String,
    file_name: String,
) -> Result<ReadFileResult, AppError> {
    let workbook = read_workbook_from_reader(cursor)?;
    validate_workbook_before_projection(&workbook)?;
    let file_data = DocumentData {
        path,
        file_name,
        sheets: ProjectionMapper::sheets_from_workbook(&workbook),
    };
    validate_file_data(&file_data)?;

    Ok(ReadFileResult {
        file_data,
        workbook: Some(workbook),
    })
}

fn validate_workbook_before_projection(workbook: &Workbook) -> Result<(), AppError> {
    if workbook.sheet_count() > MAX_WORKBOOK_SHEETS {
        return Err(AppError::ResourceLimitExceeded(format!(
            "workbook sheets is {}, maximum is {}",
            workbook.sheet_count(),
            MAX_WORKBOOK_SHEETS
        )));
    }

    let mut total_rows = 0usize;
    let mut total_slots = 0usize;
    for worksheet in workbook.sheet_collection() {
        let mut row_lengths = HashMap::<usize, usize>::new();
        let mut row_count = 0usize;
        for cell in worksheet.cells() {
            let row = cell.coordinate().row_num().saturating_sub(1) as usize;
            let col = cell.coordinate().col_num().saturating_sub(1) as usize;
            validate_position(row, col)?;
            if cell.cell_value().is_empty() {
                continue;
            }
            row_count = row_count.max(row + 1);
            row_lengths
                .entry(row)
                .and_modify(|length| *length = (*length).max(col + 1))
                .or_insert(col + 1);
        }
        total_rows = total_rows.saturating_add(row_count);
        total_slots = total_slots.saturating_add(row_lengths.into_values().sum::<usize>());
        if row_count > MAX_ROWS_PER_SHEET
            || total_rows > MAX_TOTAL_ROWS
            || total_slots > MAX_DENSE_CELL_SLOTS
        {
            return Err(AppError::ResourceLimitExceeded(
                "workbook projection is too large to load safely".to_string(),
            ));
        }
    }
    Ok(())
}

fn read_workbook_from_reader(cursor: Cursor<Vec<u8>>) -> Result<Workbook, AppError> {
    reader::xlsx::read_reader(cursor, true).map_err(|e| AppError::ReadError(e.to_string()))
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
) -> Result<ReadFileResult, AppError> {
    let mut reader = ReaderBuilder::new().has_headers(false).from_reader(cursor);

    let mut rows: Vec<Vec<CellValue>> = Vec::new();
    let mut total_slots = 0usize;

    for result in reader.records() {
        let record = result.map_err(|e| AppError::ReadError(e.to_string()))?;
        validate_position(rows.len(), record.len().saturating_sub(1))?;
        total_slots = total_slots.saturating_add(record.len());
        if total_slots > MAX_DENSE_CELL_SLOTS {
            return Err(AppError::ResourceLimitExceeded(format!(
                "CSV cell slots exceed the maximum of {MAX_DENSE_CELL_SLOTS}"
            )));
        }
        let row: Vec<CellValue> = record
            .iter()
            .map(|field| {
                if field.is_empty() {
                    CellValue::Null
                } else if has_leading_zero(field) {
                    CellValue::String(field.to_string())
                } else if let Ok(int_val) = field.parse::<i64>() {
                    if !(-9007199254740991..=9007199254740991).contains(&int_val) {
                        return CellValue::String(field.to_string());
                    }
                    CellValue::Number(CellNumber::from(int_val))
                } else if let Ok(num) = field.parse::<f64>() {
                    if num.is_finite() {
                        CellValue::Number(CellNumber::from_f64(num).expect("finite CSV number"))
                    } else {
                        CellValue::String(field.to_string())
                    }
                } else if field.eq_ignore_ascii_case("true") {
                    CellValue::Boolean(true)
                } else if field.eq_ignore_ascii_case("false") {
                    CellValue::Boolean(false)
                } else {
                    CellValue::String(field.to_string())
                }
            })
            .collect();
        rows.push(row);
    }

    let file_data = DocumentData {
        path,
        file_name,
        sheets: vec![DocumentSheet {
            name: "Sheet1".to_string(),
            rows,
            ..Default::default()
        }],
    };
    validate_file_data(&file_data)?;
    Ok(ReadFileResult {
        file_data,
        workbook: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_preflight_reserves_projection_work_before_parsing() {
        let bytes = b"alpha,beta\n";
        let preflight = preflight_input_file("csv", bytes).expect("CSV preflight");

        assert_eq!(
            preflight.estimated_parse_bytes(),
            bytes.len() * CSV_PARSE_MEMORY_MULTIPLIER
        );
    }

    #[test]
    fn xlsx_preflight_estimate_accounts_for_archive_expansion() {
        let input_bytes = 1024 * 1024;
        let uncompressed_bytes = 64 * 1024 * 1024;

        assert_eq!(
            estimate_xlsx_parse_bytes(input_bytes, uncompressed_bytes),
            input_bytes + uncompressed_bytes as usize * XLSX_UNCOMPRESSED_MEMORY_MULTIPLIER
        );
    }

    #[test]
    fn rejects_unsupported_excel_formats() {
        let result = read_file_with_workbook_from_bytes(
            "ods",
            Vec::new(),
            String::new(),
            "unsupported.ods".to_string(),
        );

        assert!(matches!(result, Err(AppError::UnsupportedFormat)));
    }
}
