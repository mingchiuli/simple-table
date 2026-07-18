use crate::document_data::{DocumentData, DocumentSheet};
use std::io::Write;
use std::str::FromStr;

use crate::document_format::{
    SpreadsheetFileFormat, file_name_from_path_like, file_stem_from_path_like,
};
use crate::error::AppError;
use crate::io::layout_units::{px_to_excel_column_width, px_to_points};
use crate::types::CellValue;
use umya_spreadsheet::{CellErrorType, Workbook, Worksheet, new_file, writer};

const DEFAULT_SHEET_NAME: &str = "Sheet1";
pub(crate) const MAX_GENERATED_FILE_BYTES: usize = 192 * 1024 * 1024;

struct LimitedBuffer {
    bytes: Vec<u8>,
    maximum_bytes: usize,
    limit_exceeded: bool,
}

impl LimitedBuffer {
    fn new(maximum_bytes: usize) -> Self {
        Self {
            bytes: Vec::new(),
            maximum_bytes,
            limit_exceeded: false,
        }
    }

    fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    fn limit_error(&self) -> AppError {
        AppError::ResourceLimitExceeded(format!(
            "generated file exceeds the maximum of {} bytes",
            self.maximum_bytes
        ))
    }
}

impl Write for LimitedBuffer {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len().saturating_add(buffer.len()) > self.maximum_bytes {
            self.limit_exceeded = true;
            return Err(std::io::Error::other("generated file byte limit exceeded"));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// 根据目标文件名/路径生成对应格式的字节。
pub fn generate_file_bytes_for_target(
    file_data: &DocumentData,
    target_path_or_name: &str,
) -> Result<(String, Vec<u8>), AppError> {
    let target_name = file_name_from_path_like(target_path_or_name, target_path_or_name);
    generate_file_bytes_for_name(file_data, &target_name)
}

fn generate_file_bytes_for_name(
    file_data: &DocumentData,
    output_name: &str,
) -> Result<(String, Vec<u8>), AppError> {
    let format = SpreadsheetFileFormat::from_path_or_default(output_name)
        .ok_or(AppError::UnsupportedFormat)?;
    let extension = format.extension();
    let output_stem = file_stem_from_path_like(output_name, "untitled");

    match format {
        SpreadsheetFileFormat::Xlsx => {
            let workbook = workbook_from_file_data(file_data)?;
            let bytes = write_workbook_to_bytes(&workbook)?;
            Ok((format!("{output_stem}.{extension}"), bytes))
        }
        SpreadsheetFileFormat::Csv => {
            let bytes = write_csv_to_bytes(file_data)?;
            Ok((format!("{output_stem}.csv"), bytes))
        }
    }
}

/// 在已有 umya Workbook 上同步当前 DocumentData，再按目标文件名生成 Excel 字节。
pub fn generate_excel_bytes_from_workbook_for_target(
    workbook: &Workbook,
    target_path_or_name: &str,
) -> Result<(String, Vec<u8>), AppError> {
    let target_name = file_name_from_path_like(target_path_or_name, target_path_or_name);
    let format = SpreadsheetFileFormat::from_path_or_default(&target_name)
        .ok_or(AppError::UnsupportedFormat)?;
    let extension = format.extension();
    let output_stem = file_stem_from_path_like(&target_name, "untitled");

    if format != SpreadsheetFileFormat::Xlsx {
        return Err(AppError::UnsupportedFormat);
    }

    let bytes = write_workbook_to_bytes(workbook)?;
    Ok((format!("{output_stem}.{extension}"), bytes))
}

pub fn workbook_from_file_data(file_data: &DocumentData) -> Result<Workbook, AppError> {
    let mut workbook = new_file();
    sync_workbook_from_file_data(&mut workbook, file_data)?;
    Ok(workbook)
}

pub fn sync_workbook_from_file_data(
    workbook: &mut Workbook,
    file_data: &DocumentData,
) -> Result<(), AppError> {
    if file_data.sheets.is_empty() {
        return Ok(());
    }

    while workbook.sheet_count() < file_data.sheets.len() {
        let sheet_index = workbook.sheet_count();
        let sheet_name = file_data
            .sheets
            .get(sheet_index)
            .map(|sheet| normalized_sheet_name(&sheet.name, sheet_index))
            .unwrap_or_else(|| normalized_sheet_name("", sheet_index));
        workbook
            .new_sheet(sheet_name)
            .map_err(|e| AppError::WriteError(e.to_string()))?;
    }

    while workbook.sheet_count() > file_data.sheets.len() && workbook.sheet_count() > 1 {
        workbook
            .remove_sheet(workbook.sheet_count() - 1)
            .map_err(|e| AppError::WriteError(e.to_string()))?;
    }

    for (sheet_index, sheet) in file_data.sheets.iter().enumerate() {
        let worksheet = workbook
            .sheet_mut(sheet_index)
            .map_err(|e| AppError::WriteError(e.to_string()))?;
        worksheet.set_name(normalized_sheet_name(&sheet.name, sheet_index));
        sync_sheet_from_sheet_data(worksheet, sheet)?;
    }

    Ok(())
}

pub fn write_workbook_to_bytes(workbook: &Workbook) -> Result<Vec<u8>, AppError> {
    let mut buffer = LimitedBuffer::new(MAX_GENERATED_FILE_BYTES);
    let result = writer::xlsx::write_writer(workbook, &mut buffer);
    if buffer.limit_exceeded {
        return Err(buffer.limit_error());
    }
    result.map_err(|e| AppError::WriteError(e.to_string()))?;
    Ok(buffer.into_bytes())
}

pub fn sync_sheet_from_sheet_data(
    worksheet: &mut Worksheet,
    sheet: &DocumentSheet,
) -> Result<(), AppError> {
    let target_column_widths = sheet.column_widths.as_ref();
    worksheet.column_dimensions_mut().retain(|column| {
        target_column_widths.is_some_and(|widths| {
            widths.contains_key(&(column.col_num().saturating_sub(1) as usize))
        })
    });

    let target_row_heights = sheet.row_heights.as_ref();
    worksheet
        .row_dimensions_to_hashmap_mut()
        .retain(|row_num, _| {
            target_row_heights
                .is_some_and(|heights| heights.contains_key(&(row_num.saturating_sub(1) as usize)))
        });

    for (col_idx, width) in sheet.column_widths.iter().flatten() {
        worksheet
            .column_dimension_by_number_mut(*col_idx as u32 + 1)
            .set_width(px_to_excel_column_width(*width));
    }

    for (row_idx, height) in sheet.row_heights.iter().flatten() {
        worksheet
            .row_dimension_mut(*row_idx as u32 + 1)
            .set_height(px_to_points(*height));
    }

    for (row_idx, row) in sheet.rows.iter().enumerate() {
        for (col_idx, cell) in row.iter().enumerate() {
            write_cell(worksheet, row_idx as u32 + 1, col_idx as u32 + 1, cell);
        }
    }

    clear_cells_outside_sheet_data(worksheet, sheet);
    worksheet.merge_cells_mut().clear();

    for merge in &sheet.merges {
        let range = format!(
            "{}:{}",
            coordinate(merge.start_col as u32 + 1, merge.start_row + 1),
            coordinate(merge.end_col as u32 + 1, merge.end_row + 1)
        );
        worksheet.add_merge_cells(range);
    }

    Ok(())
}

pub fn write_cell(worksheet: &mut Worksheet, row: u32, col: u32, cell: &CellValue) {
    let cell_ref = worksheet.cell_mut((col, row));
    match cell {
        CellValue::Formula {
            formula,
            cached_value,
            error,
        } => {
            cell_ref.set_formula(formula.trim_start_matches('='));
            if let Some(error) = error {
                if let Ok(error_type) = CellErrorType::from_str(error) {
                    cell_ref.set_formula_result_error(error_type);
                } else {
                    cell_ref.set_formula_result_string(error);
                }
            } else {
                write_formula_cached_value(cell_ref, cached_value);
            }
        }
        CellValue::String(s) => {
            cell_ref.set_value_string(s);
        }
        CellValue::Number(n) => {
            if let Some(num) = n.as_i64() {
                const F64_SAFE_MAX: i64 = 9_007_199_254_740_991;
                const F64_SAFE_MIN: i64 = -9_007_199_254_740_991;
                if (F64_SAFE_MIN..=F64_SAFE_MAX).contains(&num) {
                    cell_ref.set_value_number(num as f64);
                } else {
                    cell_ref.set_value_string(num.to_string());
                }
            } else if let Some(num) = n.as_f64() {
                if num.is_finite() {
                    cell_ref.set_value_number(num);
                }
            } else {
                cell_ref.set_value_string(n.to_string());
            }
        }
        CellValue::Boolean(b) => {
            cell_ref.set_value_bool(*b);
        }
        CellValue::Null => {
            cell_ref.set_blank();
        }
    }
}

fn clear_cells_outside_sheet_data(worksheet: &mut Worksheet, sheet: &DocumentSheet) {
    let existing_cells: Vec<(u32, u32)> = worksheet
        .cells()
        .iter()
        .map(|cell| (cell.coordinate().col_num(), cell.coordinate().row_num()))
        .collect();

    for (col, row) in existing_cells {
        let in_file_data = (row as usize)
            .checked_sub(1)
            .and_then(|row_idx| sheet.rows.get(row_idx))
            .and_then(|row_data| {
                (col as usize)
                    .checked_sub(1)
                    .and_then(|col_idx| row_data.get(col_idx))
            })
            .is_some();
        if !in_file_data {
            worksheet.cell_mut((col, row)).set_blank();
        }
    }
}

fn write_formula_cached_value(cell: &mut umya_spreadsheet::Cell, value: &CellValue) {
    match value {
        CellValue::String(s) => {
            cell.set_formula_result_string(s);
        }
        CellValue::Number(n) => {
            if let Some(num) = n.as_i64() {
                cell.set_formula_result_number(num as f64);
            } else if let Some(num) = n.as_f64()
                && num.is_finite()
            {
                cell.set_formula_result_number(num);
            } else {
                cell.set_formula_result_blank();
            }
        }
        CellValue::Boolean(b) => {
            cell.set_formula_result_bool(*b);
        }
        CellValue::Formula { cached_value, .. } => {
            write_formula_cached_value(cell, cached_value);
        }
        CellValue::Null => {
            cell.set_formula_result_blank();
        }
    }
}

pub fn coordinate(col: u32, row: u32) -> String {
    let mut col_num = col;
    let mut letters = String::new();
    while col_num > 0 {
        let rem = ((col_num - 1) % 26) as u8;
        letters.insert(0, (b'A' + rem) as char);
        col_num = (col_num - 1) / 26;
    }
    format!("{letters}{row}")
}

fn normalized_sheet_name(name: &str, sheet_index: usize) -> String {
    if name.is_empty() {
        if sheet_index == 0 {
            DEFAULT_SHEET_NAME.to_string()
        } else {
            format!("Sheet{}", sheet_index + 1)
        }
    } else {
        name.to_string()
    }
}

fn write_csv_to_bytes(file_data: &DocumentData) -> Result<Vec<u8>, AppError> {
    let mut buffer = LimitedBuffer::new(MAX_GENERATED_FILE_BYTES);
    let result = {
        let mut writer = csv::Writer::from_writer(&mut buffer);
        (|| -> Result<(), String> {
            if let Some(first_sheet) = file_data.sheets.first() {
                for row in &first_sheet.rows {
                    let string_row: Vec<String> = row.iter().map(cell_to_csv_string).collect();
                    writer
                        .write_record(&string_row)
                        .map_err(|error| error.to_string())?;
                }
            }
            writer.flush().map_err(|error| error.to_string())?;
            Ok(())
        })()
    };
    if buffer.limit_exceeded {
        return Err(buffer.limit_error());
    }
    result.map_err(AppError::WriteError)?;
    Ok(buffer.into_bytes())
}

fn cell_to_csv_string(cell: &CellValue) -> String {
    match cell {
        CellValue::Formula { .. } => cell.to_display_string(),
        CellValue::String(s) => s.clone(),
        CellValue::Number(n) => {
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
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::Value;

    use super::*;
    use crate::io::codec::reader::read_file_with_workbook_from_bytes;
    use crate::types::MergeRange;

    #[test]
    fn limited_output_buffer_rejects_bytes_before_growing_past_its_limit() {
        let mut buffer = LimitedBuffer::new(4);

        buffer.write_all(b"1234").expect("within limit");
        assert!(buffer.write_all(b"5").is_err());
        assert_eq!(buffer.bytes, b"1234");
        assert!(buffer.limit_exceeded);
    }

    #[test]
    fn preserves_formula_in_merged_top_left_cell() {
        let file_data = DocumentData {
            path: String::new(),
            file_name: "merged-formula.xlsx".to_string(),
            sheets: vec![DocumentSheet {
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
        let read_back = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "merged-formula.xlsx".to_string(),
        )
        .expect("read xlsx")
        .file_data;

        match &read_back.sheets[0].rows[0][0] {
            CellValue::Formula { formula, .. } => assert_eq!(formula, "=1+2"),
            value => panic!("expected formula, got {value:?}"),
        }
        assert_eq!(read_back.sheets[0].merges.len(), 1);
    }

    #[test]
    fn roundtrips_row_heights_and_column_widths() {
        let mut column_widths = HashMap::new();
        column_widths.insert(0, 180);
        let mut row_heights = HashMap::new();
        row_heights.insert(1, 96);

        let file_data = DocumentData {
            path: String::new(),
            file_name: "layout.xlsx".to_string(),
            sheets: vec![DocumentSheet {
                name: "Sheet1".to_string(),
                rows: vec![vec![CellValue::String("layout".to_string())]],
                column_widths: Some(column_widths),
                row_heights: Some(row_heights),
                ..Default::default()
            }],
        };

        let (_, bytes) =
            generate_file_bytes_for_target(&file_data, "layout.xlsx").expect("write xlsx");
        let read_back = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "layout.xlsx".to_string(),
        )
        .expect("read xlsx")
        .file_data;

        assert_eq!(
            read_back.sheets[0]
                .column_widths
                .as_ref()
                .and_then(|widths| widths.get(&0)),
            Some(&180)
        );
        assert_eq!(
            read_back.sheets[0]
                .row_heights
                .as_ref()
                .and_then(|heights| heights.get(&1)),
            Some(&96)
        );
    }

    #[test]
    fn sync_sheet_removes_stale_row_heights_and_column_widths() {
        let mut workbook = new_file();
        {
            let sheet = workbook.sheet_mut(0).expect("sheet");
            sheet.column_dimension_by_number_mut(1).set_width(25.0);
            sheet.column_dimension_by_number_mut(2).set_width(30.0);
            sheet.row_dimension_mut(1).set_height(72.0);
            sheet.row_dimension_mut(2).set_height(96.0);
        }

        let mut column_widths = HashMap::new();
        column_widths.insert(1, 210);
        let mut row_heights = HashMap::new();
        row_heights.insert(1, 120);
        let sheet_data = DocumentSheet {
            name: "Sheet1".to_string(),
            rows: vec![vec![CellValue::String("layout".to_string())]],
            column_widths: Some(column_widths),
            row_heights: Some(row_heights),
            ..Default::default()
        };

        sync_sheet_from_sheet_data(workbook.sheet_mut(0).expect("sheet"), &sheet_data)
            .expect("sync sheet");
        let bytes = write_workbook_to_bytes(&workbook).expect("write workbook");
        let read_back = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "layout-cleanup.xlsx".to_string(),
        )
        .expect("read workbook")
        .file_data;
        let sheet = &read_back.sheets[0];

        assert_eq!(
            sheet
                .column_widths
                .as_ref()
                .and_then(|widths| widths.get(&0)),
            None
        );
        assert_eq!(
            sheet
                .column_widths
                .as_ref()
                .and_then(|widths| widths.get(&1)),
            Some(&210)
        );
        assert_eq!(
            sheet
                .row_heights
                .as_ref()
                .and_then(|heights| heights.get(&0)),
            None
        );
        assert_eq!(
            sheet
                .row_heights
                .as_ref()
                .and_then(|heights| heights.get(&1)),
            Some(&120)
        );
    }

    #[test]
    fn output_name_uses_decoded_path_like_target_name() {
        let file_data = DocumentData {
            path: String::new(),
            file_name: "source.xlsx".to_string(),
            sheets: vec![DocumentSheet {
                name: "Sheet1".to_string(),
                rows: vec![vec![CellValue::String("ok".to_string())]],
                ..Default::default()
            }],
        };

        let (output_name, _) = generate_file_bytes_for_target(
            &file_data,
            "content://provider/document/primary%3ADownload%2Freports%2Fscore.final.xlsx?token=1",
        )
        .expect("write path-like target");

        assert_eq!(output_name, "score.final.xlsx");
    }

    #[test]
    fn rejects_unsupported_output_extension() {
        let file_data = DocumentData {
            path: String::new(),
            file_name: "unsupported.bin".to_string(),
            sheets: vec![DocumentSheet {
                name: "Sheet1".to_string(),
                rows: vec![vec![CellValue::String("ok".to_string())]],
                ..Default::default()
            }],
        };

        assert!(matches!(
            generate_file_bytes_for_target(&file_data, "unsupported.bin"),
            Err(AppError::UnsupportedFormat)
        ));
    }
}
