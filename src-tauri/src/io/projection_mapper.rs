use crate::error::AppError;
use crate::io::codec::reader::read_worksheet;
use crate::io::codec::writer::coordinate;
use crate::types::{CellValue, FileData, SheetData};
use umya_spreadsheet::{Workbook, Worksheet};

pub(crate) struct ProjectionMapper;

impl ProjectionMapper {
    pub(crate) fn sheets_from_workbook(workbook: &Workbook) -> Vec<SheetData> {
        workbook
            .sheet_collection()
            .iter()
            .map(read_worksheet)
            .collect()
    }

    pub(crate) fn refresh_file_data_from_workbook(workbook: &Workbook, file_data: &mut FileData) {
        file_data.sheets = Self::sheets_from_workbook(workbook);
    }

    pub(crate) fn sync_merge_ranges_to_workbook(
        workbook: &mut Workbook,
        file_data: &FileData,
    ) -> Result<(), AppError> {
        for sheet_index in 0..file_data.sheets.len() {
            let Some(worksheet) = sheet_mut(workbook, sheet_index)? else {
                continue;
            };
            worksheet.merge_cells_mut().clear();
            let Some(sheet) = file_data.sheets.get(sheet_index) else {
                continue;
            };
            for merge in &sheet.merges {
                let range = format!(
                    "{}:{}",
                    coordinate(merge.start_col as u32 + 1, merge.start_row + 1),
                    coordinate(merge.end_col as u32 + 1, merge.end_row + 1)
                );
                worksheet.add_merge_cells(range);
            }
        }
        Ok(())
    }

    pub(crate) fn validate_workbook_matches_projection(
        workbook: &Workbook,
        projection: &FileData,
    ) -> Result<(), AppError> {
        if workbook.sheet_count() != projection.sheets.len() {
            return Err(AppError::Internal(format!(
                "workbook/projection sheet count mismatch: workbook={}, projection={}",
                workbook.sheet_count(),
                projection.sheets.len()
            )));
        }

        for (sheet_index, worksheet) in workbook.sheet_collection().iter().enumerate() {
            let actual = read_worksheet(worksheet);
            let Some(expected) = projection.sheets.get(sheet_index) else {
                return Err(AppError::Internal(format!(
                    "projection is missing sheet {sheet_index}"
                )));
            };
            if !sheets_are_consistent(expected, &actual) {
                return Err(AppError::Internal(format!(
                    "workbook/projection mismatch on sheet {sheet_index} ({}): {}",
                    expected.name,
                    sheet_difference(expected, &actual)
                )));
            }
        }

        Ok(())
    }
}

fn sheet_mut(
    workbook: &mut Workbook,
    sheet_index: usize,
) -> Result<Option<&mut Worksheet>, AppError> {
    if sheet_index >= workbook.sheet_count() {
        return Ok(None);
    }
    workbook
        .sheet_mut(sheet_index)
        .map(Some)
        .map_err(|e| AppError::WriteError(e.to_string()))
}

fn sheet_difference(expected: &SheetData, actual: &SheetData) -> String {
    if expected.name != actual.name {
        return format!(
            "name differs: projection={:?}, workbook={:?}",
            expected.name, actual.name
        );
    }
    if !rows_are_consistent(&expected.rows, &actual.rows) {
        return row_difference(&expected.rows, &actual.rows);
    }
    if expected.merges != actual.merges {
        return format!(
            "merges differ: projection={}, workbook={}",
            expected.merges.len(),
            actual.merges.len()
        );
    }
    if expected.column_widths != actual.column_widths {
        return "column widths differ".to_string();
    }
    if expected.row_heights != actual.row_heights {
        return "row heights differ".to_string();
    }
    if expected.rich != actual.rich {
        return "rich projection differs".to_string();
    }
    "unknown difference".to_string()
}

fn sheets_are_consistent(expected: &SheetData, actual: &SheetData) -> bool {
    expected.name == actual.name
        && rows_are_consistent(&expected.rows, &actual.rows)
        && expected.merges == actual.merges
        && expected.column_widths == actual.column_widths
        && expected.row_heights == actual.row_heights
        && expected.rich == actual.rich
}

fn rows_are_consistent(expected: &[Vec<CellValue>], actual: &[Vec<CellValue>]) -> bool {
    let expected_rows = effective_row_count(expected);
    let actual_rows = effective_row_count(actual);
    expected_rows == actual_rows
        && (0..expected_rows).all(|row_index| {
            let expected = &expected[row_index];
            let actual = &actual[row_index];
            let expected_len = effective_row_len(expected);
            let actual_len = effective_row_len(actual);
            expected_len == actual_len
                && (0..expected_len)
                    .all(|col_index| cells_are_consistent(&expected[col_index], &actual[col_index]))
        })
}

fn effective_row_count(rows: &[Vec<CellValue>]) -> usize {
    rows.iter()
        .rposition(|row| effective_row_len(row) > 0)
        .map(|index| index + 1)
        .unwrap_or(0)
}

fn effective_row_len(row: &[CellValue]) -> usize {
    row.iter()
        .rposition(|cell| !matches!(cell, CellValue::Null))
        .map(|index| index + 1)
        .unwrap_or(0)
}

fn cells_are_consistent(expected: &CellValue, actual: &CellValue) -> bool {
    match (expected, actual) {
        (CellValue::Null, CellValue::Null) => true,
        (CellValue::String(expected), CellValue::String(actual)) => expected == actual,
        (CellValue::Boolean(expected), CellValue::Boolean(actual)) => expected == actual,
        (CellValue::Number(expected), CellValue::Number(actual)) => {
            match (expected.as_f64(), actual.as_f64()) {
                (Some(expected), Some(actual)) => (expected - actual).abs() < 0.000_000_1,
                _ => expected == actual,
            }
        }
        (
            CellValue::Formula {
                formula: expected_formula,
                cached_value: expected_cached,
                error: expected_error,
            },
            CellValue::Formula {
                formula: actual_formula,
                cached_value: actual_cached,
                error: actual_error,
            },
        ) => {
            expected_formula == actual_formula
                && formula_results_are_consistent(
                    expected_cached,
                    expected_error.as_deref(),
                    actual_cached,
                    actual_error.as_deref(),
                )
        }
        _ => false,
    }
}

fn formula_results_are_consistent(
    expected_cached: &CellValue,
    expected_error: Option<&str>,
    actual_cached: &CellValue,
    actual_error: Option<&str>,
) -> bool {
    if expected_error == actual_error {
        return true;
    }
    if let Some(expected_error) = expected_error
        && actual_error.is_none()
        && matches!(actual_cached, CellValue::String(value) if value == expected_error)
    {
        return true;
    }
    if let Some(actual_error) = actual_error
        && expected_error.is_none()
        && matches!(expected_cached, CellValue::String(value) if value == actual_error)
    {
        return true;
    }
    cells_are_consistent(expected_cached, actual_cached)
}

fn row_difference(expected: &[Vec<CellValue>], actual: &[Vec<CellValue>]) -> String {
    let expected_rows = effective_row_count(expected);
    let actual_rows = effective_row_count(actual);
    if expected_rows != actual_rows {
        return format!(
            "row count differs: projection={}, workbook={}",
            expected_rows, actual_rows
        );
    }
    for row_index in 0..expected_rows {
        let expected_row = &expected[row_index];
        let actual_row = &actual[row_index];
        let expected_len = effective_row_len(expected_row);
        let actual_len = effective_row_len(actual_row);
        if expected_len != actual_len {
            return format!(
                "row {row_index} width differs: projection={}, workbook={}",
                expected_len, actual_len
            );
        }
        for col_index in 0..expected_len {
            if !cells_are_consistent(&expected_row[col_index], &actual_row[col_index]) {
                return format!(
                    "cell ({row_index},{col_index}) differs: projection={:?}, workbook={:?}",
                    expected_row[col_index], actual_row[col_index]
                );
            }
        }
    }
    "rows differ".to_string()
}
