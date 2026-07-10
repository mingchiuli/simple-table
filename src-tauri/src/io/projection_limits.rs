use std::collections::HashMap;

use crate::error::AppError;
use crate::io::rich_projection::parse_cell_key;
use crate::types::{CellValue, FileData, SheetData};

pub const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_XLSX_UNCOMPRESSED_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_XLSX_ARCHIVE_ENTRIES: usize = 20_000;
pub const MAX_WORKBOOK_SHEETS: usize = 256;
pub const MAX_ROWS_PER_SHEET: usize = 250_000;
pub const MAX_TOTAL_ROWS: usize = 500_000;
pub const MAX_COLUMNS_PER_ROW: usize = 16_384;
pub const MAX_DENSE_CELL_SLOTS: usize = 2_000_000;
pub const MAX_RICH_METADATA_ENTRIES: usize = 1_000_000;
pub const MAX_CELL_TEXT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_MUTATION_TEXT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PROJECTED_TEXT_BYTES: usize = 64 * 1024 * 1024;

pub fn validate_input_size(byte_count: usize) -> Result<(), AppError> {
    ensure_limit("file bytes", byte_count, MAX_INPUT_BYTES)
}

pub fn validate_input_file_size(byte_count: u64) -> Result<(), AppError> {
    if byte_count > MAX_INPUT_BYTES as u64 {
        return Err(limit_error(format!(
            "file bytes is {byte_count}, maximum is {MAX_INPUT_BYTES}"
        )));
    }
    Ok(())
}

pub fn validate_file_data(file_data: &FileData) -> Result<(), AppError> {
    ensure_limit(
        "workbook sheets",
        file_data.sheets.len(),
        MAX_WORKBOOK_SHEETS,
    )?;
    let usage = projection_usage(file_data)?;
    validate_usage(usage)
}

pub fn validate_cell_changes<'a>(
    file_data: &FileData,
    changes: impl IntoIterator<Item = (usize, usize, usize, &'a CellValue, &'a CellValue)>,
) -> Result<(), AppError> {
    let usage = projection_usage(file_data)?;
    let mut projected_row_lengths = HashMap::<(usize, usize), usize>::new();
    let mut projected_sheet_rows: Vec<usize> = file_data
        .sheets
        .iter()
        .map(|sheet| sheet.rows.len())
        .collect();
    let mut dense_slots = usage.dense_slots;
    let mut total_rows = usage.total_rows;
    let mut text_bytes = usage.text_bytes;
    let mut mutation_text_bytes = 0usize;

    for (sheet_index, row, col, old_value, new_value) in changes {
        validate_position(row, col)?;
        let sheet = file_data
            .sheets
            .get(sheet_index)
            .ok_or(AppError::InvalidSheetIndex(sheet_index))?;
        let current_length = projected_row_lengths
            .get(&(sheet_index, row))
            .copied()
            .unwrap_or_else(|| sheet.rows.get(row).map(Vec::len).unwrap_or_default());
        let projected_length = current_length.max(col + 1);
        dense_slots = checked_add(dense_slots, projected_length - current_length, "cell slots")?;
        projected_row_lengths.insert((sheet_index, row), projected_length);

        let required_rows = row + 1;
        if required_rows > projected_sheet_rows[sheet_index] {
            let added_rows = required_rows - projected_sheet_rows[sheet_index];
            total_rows = checked_add(total_rows, added_rows, "rows")?;
            projected_sheet_rows[sheet_index] = required_rows;
        }

        let old_text_bytes = cell_text_bytes(old_value);
        let new_text_bytes = cell_text_bytes(new_value);
        ensure_limit("cell text bytes", new_text_bytes, MAX_CELL_TEXT_BYTES)?;
        mutation_text_bytes =
            checked_add(mutation_text_bytes, new_text_bytes, "mutation text bytes")?;
        text_bytes = text_bytes
            .checked_sub(old_text_bytes)
            .and_then(|remaining| remaining.checked_add(new_text_bytes))
            .ok_or_else(|| limit_error("projected text bytes overflowed".to_string()))?;
    }
    ensure_limit(
        "mutation text bytes",
        mutation_text_bytes,
        MAX_MUTATION_TEXT_BYTES,
    )?;

    validate_usage(ProjectionUsage {
        dense_slots,
        total_rows,
        rich_entries: usage.rich_entries,
        text_bytes,
    })
}

pub fn validate_added_row(
    file_data: &FileData,
    sheet: &SheetData,
    projected_sheet_rows: usize,
    row_width: usize,
) -> Result<(), AppError> {
    ensure_limit("rows per sheet", projected_sheet_rows, MAX_ROWS_PER_SHEET)?;
    ensure_limit("columns per row", row_width, MAX_COLUMNS_PER_ROW)?;
    let usage = projection_usage(file_data)?;
    let added_rows = projected_sheet_rows.saturating_sub(sheet.rows.len());
    validate_usage(ProjectionUsage {
        dense_slots: checked_add(usage.dense_slots, row_width, "cell slots")?,
        total_rows: checked_add(usage.total_rows, added_rows, "rows")?,
        rich_entries: usage.rich_entries,
        text_bytes: usage.text_bytes,
    })
}

pub fn validate_added_column(
    file_data: &FileData,
    sheet: &SheetData,
    projected_row_count: usize,
    col_index: usize,
) -> Result<(), AppError> {
    validate_position(projected_row_count.saturating_sub(1), col_index)?;
    let usage = projection_usage(file_data)?;
    let mut additional_slots = 0usize;
    for row_index in 0..projected_row_count {
        let current_length = sheet.rows.get(row_index).map(Vec::len).unwrap_or_default();
        let projected_length = if current_length < col_index {
            col_index + 1
        } else {
            current_length + 1
        };
        ensure_limit("columns per row", projected_length, MAX_COLUMNS_PER_ROW)?;
        additional_slots = checked_add(
            additional_slots,
            projected_length - current_length,
            "cell slots",
        )?;
    }
    let added_rows = projected_row_count.saturating_sub(sheet.rows.len());
    validate_usage(ProjectionUsage {
        dense_slots: checked_add(usage.dense_slots, additional_slots, "cell slots")?,
        total_rows: checked_add(usage.total_rows, added_rows, "rows")?,
        rich_entries: usage.rich_entries,
        text_bytes: usage.text_bytes,
    })
}

pub fn validate_added_sheet(file_data: &FileData) -> Result<(), AppError> {
    ensure_limit(
        "workbook sheets",
        file_data.sheets.len().saturating_add(1),
        MAX_WORKBOOK_SHEETS,
    )
}

pub fn validate_position(row: usize, col: usize) -> Result<(), AppError> {
    if row >= MAX_ROWS_PER_SHEET || col >= MAX_COLUMNS_PER_ROW {
        return Err(limit_error(format!(
            "cell position row {}, column {} exceeds row/column limits ({}, {})",
            row + 1,
            col + 1,
            MAX_ROWS_PER_SHEET,
            MAX_COLUMNS_PER_ROW
        )));
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct ProjectionUsage {
    dense_slots: usize,
    total_rows: usize,
    rich_entries: usize,
    text_bytes: usize,
}

fn projection_usage(file_data: &FileData) -> Result<ProjectionUsage, AppError> {
    let mut usage = ProjectionUsage {
        dense_slots: 0,
        total_rows: 0,
        rich_entries: 0,
        text_bytes: 0,
    };
    for sheet in &file_data.sheets {
        ensure_limit("rows per sheet", sheet.rows.len(), MAX_ROWS_PER_SHEET)?;
        usage.total_rows = checked_add(usage.total_rows, sheet.rows.len(), "rows")?;
        for row in &sheet.rows {
            ensure_limit("columns per row", row.len(), MAX_COLUMNS_PER_ROW)?;
            usage.dense_slots = checked_add(usage.dense_slots, row.len(), "cell slots")?;
            for cell in row {
                let cell_bytes = cell_text_bytes(cell);
                ensure_limit("cell text bytes", cell_bytes, MAX_CELL_TEXT_BYTES)?;
                usage.text_bytes = checked_add(usage.text_bytes, cell_bytes, "text bytes")?;
            }
        }
        validate_sheet_metadata(sheet, &mut usage)?;
    }
    Ok(usage)
}

fn validate_sheet_metadata(sheet: &SheetData, usage: &mut ProjectionUsage) -> Result<(), AppError> {
    for merge in &sheet.merges {
        validate_position(merge.start_row as usize, merge.start_col as usize)?;
        validate_position(merge.end_row as usize, merge.end_col as usize)?;
    }
    for index in sheet.column_widths.iter().flat_map(|values| values.keys()) {
        validate_position(0, *index)?;
    }
    for index in sheet.row_heights.iter().flat_map(|values| values.keys()) {
        validate_position(*index, 0)?;
    }
    for index in &sheet.rich.hidden_rows {
        validate_position(*index, 0)?;
    }
    for index in &sheet.rich.hidden_columns {
        validate_position(0, *index)?;
    }
    for key in sheet
        .rich
        .cell_formats
        .keys()
        .chain(sheet.rich.cell_styles.keys())
        .chain(sheet.rich.hyperlinks.keys())
    {
        if let Some((row, col)) = parse_cell_key(key) {
            validate_position(row, col)?;
        }
    }
    for drawing in &sheet.rich.drawings {
        validate_position(drawing.from_row as usize, drawing.from_col as usize)?;
        if let (Some(row), Some(col)) = (drawing.to_row, drawing.to_col) {
            validate_position(row as usize, col as usize)?;
        }
    }

    let rich_entries = sheet.merges.len()
        + sheet
            .column_widths
            .as_ref()
            .map(|values| values.len())
            .unwrap_or_default()
        + sheet
            .row_heights
            .as_ref()
            .map(|values| values.len())
            .unwrap_or_default()
        + sheet.rich.cell_formats.len()
        + sheet.rich.cell_styles.len()
        + sheet.rich.hyperlinks.len()
        + sheet.rich.hidden_rows.len()
        + sheet.rich.hidden_columns.len()
        + sheet.rich.drawings.len();
    usage.rich_entries = checked_add(usage.rich_entries, rich_entries, "rich metadata entries")?;
    Ok(())
}

fn validate_usage(usage: ProjectionUsage) -> Result<(), AppError> {
    ensure_limit("rows", usage.total_rows, MAX_TOTAL_ROWS)?;
    ensure_limit("cell slots", usage.dense_slots, MAX_DENSE_CELL_SLOTS)?;
    ensure_limit(
        "rich metadata entries",
        usage.rich_entries,
        MAX_RICH_METADATA_ENTRIES,
    )?;
    ensure_limit("text bytes", usage.text_bytes, MAX_PROJECTED_TEXT_BYTES)
}

fn cell_text_bytes(cell: &CellValue) -> usize {
    match cell {
        CellValue::Null | CellValue::Boolean(_) => 0,
        CellValue::String(value) => value.len(),
        CellValue::Number(value) => value.to_string().len(),
        CellValue::Formula {
            formula,
            cached_value,
            error,
        } => {
            formula.len()
                + cell_text_bytes(cached_value)
                + error.as_ref().map(String::len).unwrap_or_default()
        }
    }
}

fn checked_add(left: usize, right: usize, label: &str) -> Result<usize, AppError> {
    left.checked_add(right)
        .ok_or_else(|| limit_error(format!("{label} overflowed the supported size")))
}

fn ensure_limit(label: &str, actual: usize, maximum: usize) -> Result<(), AppError> {
    if actual > maximum {
        return Err(limit_error(format!(
            "{label} is {actual}, maximum is {maximum}"
        )));
    }
    Ok(())
}

fn limit_error(message: String) -> AppError {
    AppError::ResourceLimitExceeded(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CellValue, SheetData};

    #[test]
    fn rejects_oversized_input_without_allocating_it() {
        let error = validate_input_size(MAX_INPUT_BYTES + 1).expect_err("oversized input");
        assert!(matches!(error, AppError::ResourceLimitExceeded(_)));

        let error = validate_input_file_size(MAX_INPUT_BYTES as u64 + 1)
            .expect_err("oversized input metadata");
        assert!(matches!(error, AppError::ResourceLimitExceeded(_)));
    }

    #[test]
    fn rejects_remote_cell_targets_before_projection_growth() {
        let file_data = FileData {
            path: String::new(),
            file_name: "book.xlsx".to_string(),
            sheets: vec![SheetData::default()],
        };

        let old_value = CellValue::Null;
        let new_value = CellValue::String("value".to_string());
        let error = validate_cell_changes(
            &file_data,
            [(0, MAX_ROWS_PER_SHEET, 0, &old_value, &new_value)],
        )
        .expect_err("remote target");

        assert!(matches!(error, AppError::ResourceLimitExceeded(_)));
    }

    #[test]
    fn accounts_for_dense_growth_across_batched_targets() {
        let file_data = FileData {
            path: String::new(),
            file_name: "book.xlsx".to_string(),
            sheets: vec![SheetData {
                rows: vec![vec![CellValue::Null]],
                ..Default::default()
            }],
        };

        let old_value = CellValue::Null;
        let first_value = CellValue::String("first".to_string());
        let second_value = CellValue::String("second".to_string());
        validate_cell_changes(
            &file_data,
            [
                (0, 0, 10, &old_value, &first_value),
                (0, 0, 20, &old_value, &second_value),
            ],
        )
        .expect("bounded targets");
    }

    #[test]
    fn rejects_projection_usage_over_the_text_budget() {
        let error = validate_usage(ProjectionUsage {
            dense_slots: 0,
            total_rows: 0,
            rich_entries: 0,
            text_bytes: MAX_PROJECTED_TEXT_BYTES + 1,
        })
        .expect_err("projected text budget");

        assert!(matches!(error, AppError::ResourceLimitExceeded(_)));
    }
}
