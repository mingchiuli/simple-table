use crate::document_data::{DocumentData, DocumentSheet, SheetExtent};
use std::collections::HashMap;

use crate::document_layout_policy::{
    MAX_COLUMN_WIDTH_PX, MAX_ROW_HEIGHT_PX, MIN_COLUMN_WIDTH_PX, MIN_ROW_HEIGHT_PX,
    is_supported_column_width, is_supported_row_height,
};
use crate::document_resource_estimator::{document_metadata_text_usage, sheet_metadata_text_usage};
use crate::domain::CellValue;
use crate::domain::cell_key::parse_cell_key;
use crate::error::AppError;

pub const MAX_WORKBOOK_SHEETS: usize = 256;
pub const MAX_ROWS_PER_SHEET: usize = 250_000;
pub const MAX_TOTAL_ROWS: usize = 500_000;
pub const MAX_COLUMNS_PER_ROW: usize = 16_384;
pub const MAX_DENSE_CELL_SLOTS: usize = 2_000_000;
pub const MAX_RICH_METADATA_ENTRIES: usize = 1_000_000;
pub const MAX_LAYOUT_OVERRIDES: usize = 100_000;
pub const MAX_CELL_TEXT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_MUTATION_TEXT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SET_CELL_CHANGES: usize = 4_096;
pub const MAX_MUTATION_RESPONSE_BYTES: usize = 3 * 1024 * 1024;
pub const MAX_DOCUMENT_RESPONSE_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_SHEET_REGION_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_SEARCH_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_SEARCH_QUERY_BYTES: usize = 4 * 1024;
pub const MAX_PROJECTED_TEXT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_METADATA_STRING_BYTES: usize = 1024 * 1024;
pub const MAX_PROJECTED_METADATA_TEXT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_DOCUMENT_PATH_BYTES: usize = 64 * 1024;
pub const MAX_FILE_NAME_BYTES: usize = 4 * 1024;
pub const MAX_SHEET_NAME_BYTES: usize = 4 * 1024;
pub const SHEET_REGION_TILE_ROWS: usize = 128;
pub const SHEET_REGION_TILE_COLUMNS: usize = 32;
pub const MAX_PREPARED_DOCUMENT_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_ACTIVE_AND_PREPARED_DOCUMENT_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_SAVE_SOURCE_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_GENERATED_FILE_BYTES: usize = 192 * 1024 * 1024;
pub const MAX_DOCUMENT_WORKING_SET_BYTES: usize = 832 * 1024 * 1024;
pub const MAX_SEARCH_OUTCOME_RETAINED_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub struct ResourceLedger {
    sheets: Vec<SheetResourceUsage>,
    total: ProjectionUsage,
    identity_metadata_bytes: usize,
}

#[derive(Clone)]
struct SheetResourceUsage {
    usage: ProjectionUsage,
    extent: SheetExtent,
}

impl ResourceLedger {
    pub fn from_file_data(file_data: &DocumentData) -> Self {
        let sheets: Vec<_> = file_data.sheets.iter().map(sheet_resource_usage).collect();
        let mut total = sheets
            .iter()
            .fold(ProjectionUsage::default(), |total, sheet| {
                total + sheet.usage
            });
        let identity_metadata_bytes = file_data
            .path
            .len()
            .saturating_add(file_data.file_name.len());
        total.metadata_text_bytes = total
            .metadata_text_bytes
            .saturating_add(identity_metadata_bytes);
        Self {
            sheets,
            total,
            identity_metadata_bytes,
        }
    }

    pub fn sheet_extents(&self) -> Vec<SheetExtent> {
        self.sheets.iter().map(|sheet| sheet.extent).collect()
    }

    pub fn sheet_extent(&self, sheet_index: usize) -> Option<SheetExtent> {
        self.sheets.get(sheet_index).map(|sheet| sheet.extent)
    }

    pub fn refresh_sheets(
        &mut self,
        file_data: &DocumentData,
        sheet_indexes: impl IntoIterator<Item = usize>,
    ) {
        if self.sheets.len() != file_data.sheets.len() {
            *self = Self::from_file_data(file_data);
            return;
        }

        let mut refreshed = std::collections::BTreeSet::new();
        for sheet_index in sheet_indexes {
            if !refreshed.insert(sheet_index) {
                continue;
            }
            let (Some(sheet), Some(previous)) = (
                file_data.sheets.get(sheet_index),
                self.sheets.get_mut(sheet_index),
            ) else {
                continue;
            };
            let next = sheet_resource_usage(sheet);
            self.total = self.total - previous.usage + next.usage;
            *previous = next;
        }
    }

    pub fn replace_all(&mut self, file_data: &DocumentData) {
        *self = Self::from_file_data(file_data);
    }

    pub fn refresh_identity(&mut self, file_data: &DocumentData) {
        let next = file_data
            .path
            .len()
            .saturating_add(file_data.file_name.len());
        self.total.metadata_text_bytes = self
            .total
            .metadata_text_bytes
            .saturating_sub(self.identity_metadata_bytes)
            .saturating_add(next);
        self.identity_metadata_bytes = next;
    }

    pub fn validate_cell_changes<'a>(
        &self,
        file_data: &DocumentData,
        changes: impl IntoIterator<Item = (usize, usize, usize, &'a CellValue, &'a CellValue)>,
    ) -> Result<(), AppError> {
        validate_cell_changes_with_usage(file_data, self.total, changes)
    }

    pub fn validate_added_row(
        &self,
        sheet: &DocumentSheet,
        projected_sheet_rows: usize,
        row_width: usize,
    ) -> Result<(), AppError> {
        validate_added_row_with_usage(self.total, sheet, projected_sheet_rows, row_width)
    }

    pub fn validate_added_column(
        &self,
        sheet: &DocumentSheet,
        projected_row_count: usize,
        col_index: usize,
    ) -> Result<(), AppError> {
        validate_added_column_with_usage(self.total, sheet, projected_row_count, col_index)
    }

    pub fn validate_layout_change(
        &self,
        had_override: bool,
        has_override: bool,
    ) -> Result<(), AppError> {
        let layout_entries = match (had_override, has_override) {
            (false, true) => checked_add(self.total.layout_entries, 1, "layout overrides")?,
            (true, false) => self.total.layout_entries.saturating_sub(1),
            _ => self.total.layout_entries,
        };
        validate_usage(ProjectionUsage {
            layout_entries,
            ..self.total
        })
    }

    pub fn estimated_bytes(&self) -> usize {
        self.total.estimated_bytes()
    }

    pub(crate) fn sheet_estimated_bytes(&self, sheet_index: usize) -> Option<usize> {
        self.sheets
            .get(sheet_index)
            .map(|sheet| sheet.usage.estimated_bytes())
    }
}

pub fn validate_column_width(width: Option<u32>) -> Result<(), AppError> {
    let Some(width) = width else {
        return Ok(());
    };
    if is_supported_column_width(width) {
        return Ok(());
    }
    Err(limit_error(format!(
        "column width must be between {MIN_COLUMN_WIDTH_PX} and {MAX_COLUMN_WIDTH_PX} pixels"
    )))
}

pub fn validate_row_height(height: Option<u32>) -> Result<(), AppError> {
    let Some(height) = height else {
        return Ok(());
    };
    if is_supported_row_height(height) {
        return Ok(());
    }
    Err(limit_error(format!(
        "row height must be between {MIN_ROW_HEIGHT_PX} and {MAX_ROW_HEIGHT_PX} pixels"
    )))
}

pub fn validate_file_data(file_data: &DocumentData) -> Result<(), AppError> {
    validate_document_identity(&file_data.path, &file_data.file_name)?;
    ensure_limit(
        "workbook sheets",
        file_data.sheets.len(),
        MAX_WORKBOOK_SHEETS,
    )?;
    let usage = projection_usage(file_data)?;
    validate_usage(usage)
}

pub fn validate_document_identity(path: &str, file_name: &str) -> Result<(), AppError> {
    ensure_limit("document path bytes", path.len(), MAX_DOCUMENT_PATH_BYTES)?;
    ensure_limit("file name bytes", file_name.len(), MAX_FILE_NAME_BYTES)
}

pub fn validate_prepared_document_bytes(estimated_bytes: usize) -> Result<(), AppError> {
    if estimated_bytes > MAX_PREPARED_DOCUMENT_BYTES {
        return Err(limit_error(format!(
            "prepared document parse requires an estimated {estimated_bytes} bytes, maximum is {MAX_PREPARED_DOCUMENT_BYTES}"
        )));
    }
    Ok(())
}

pub fn validate_active_and_prepared_document_bytes(
    active_bytes: usize,
    prepared_bytes: usize,
) -> Result<(), AppError> {
    let peak_bytes = active_bytes.saturating_add(prepared_bytes);
    if peak_bytes > MAX_ACTIVE_AND_PREPARED_DOCUMENT_BYTES {
        return Err(limit_error(format!(
            "active and prepared documents require an estimated {peak_bytes} bytes, maximum is {MAX_ACTIVE_AND_PREPARED_DOCUMENT_BYTES}"
        )));
    }
    Ok(())
}

#[cfg(test)]
pub fn validate_cell_changes<'a>(
    file_data: &DocumentData,
    changes: impl IntoIterator<Item = (usize, usize, usize, &'a CellValue, &'a CellValue)>,
) -> Result<(), AppError> {
    let usage = projection_usage(file_data)?;
    validate_cell_changes_with_usage(file_data, usage, changes)
}

fn validate_cell_changes_with_usage<'a>(
    file_data: &DocumentData,
    usage: ProjectionUsage,
    changes: impl IntoIterator<Item = (usize, usize, usize, &'a CellValue, &'a CellValue)>,
) -> Result<(), AppError> {
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
        layout_entries: usage.layout_entries,
        text_bytes,
        metadata_text_bytes: usage.metadata_text_bytes,
    })
}

fn validate_added_row_with_usage(
    usage: ProjectionUsage,
    sheet: &DocumentSheet,
    projected_sheet_rows: usize,
    row_width: usize,
) -> Result<(), AppError> {
    ensure_limit("rows per sheet", projected_sheet_rows, MAX_ROWS_PER_SHEET)?;
    ensure_limit("columns per row", row_width, MAX_COLUMNS_PER_ROW)?;
    let added_rows = projected_sheet_rows.saturating_sub(sheet.rows.len());
    validate_usage(ProjectionUsage {
        dense_slots: checked_add(usage.dense_slots, row_width, "cell slots")?,
        total_rows: checked_add(usage.total_rows, added_rows, "rows")?,
        rich_entries: usage.rich_entries,
        layout_entries: usage.layout_entries,
        text_bytes: usage.text_bytes,
        metadata_text_bytes: usage.metadata_text_bytes,
    })
}

fn validate_added_column_with_usage(
    usage: ProjectionUsage,
    sheet: &DocumentSheet,
    projected_row_count: usize,
    col_index: usize,
) -> Result<(), AppError> {
    validate_position(projected_row_count.saturating_sub(1), col_index)?;
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
        layout_entries: usage.layout_entries,
        text_bytes: usage.text_bytes,
        metadata_text_bytes: usage.metadata_text_bytes,
    })
}

pub fn validate_added_sheet(file_data: &DocumentData, sheet_name: &str) -> Result<(), AppError> {
    ensure_limit(
        "workbook sheets",
        file_data.sheets.len().saturating_add(1),
        MAX_WORKBOOK_SHEETS,
    )?;
    ensure_limit("sheet name bytes", sheet_name.len(), MAX_SHEET_NAME_BYTES)?;
    ensure_limit(
        "metadata string bytes",
        sheet_name.len(),
        MAX_METADATA_STRING_BYTES,
    )?;
    ensure_limit(
        "metadata text bytes",
        document_metadata_text_usage(file_data)
            .total_bytes
            .saturating_add(sheet_name.len()),
        MAX_PROJECTED_METADATA_TEXT_BYTES,
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

#[derive(Clone, Copy, Default)]
struct ProjectionUsage {
    dense_slots: usize,
    total_rows: usize,
    rich_entries: usize,
    layout_entries: usize,
    text_bytes: usize,
    metadata_text_bytes: usize,
}

impl std::ops::Add for ProjectionUsage {
    type Output = Self;

    fn add(self, right: Self) -> Self::Output {
        Self {
            dense_slots: self.dense_slots.saturating_add(right.dense_slots),
            total_rows: self.total_rows.saturating_add(right.total_rows),
            rich_entries: self.rich_entries.saturating_add(right.rich_entries),
            layout_entries: self.layout_entries.saturating_add(right.layout_entries),
            text_bytes: self.text_bytes.saturating_add(right.text_bytes),
            metadata_text_bytes: self
                .metadata_text_bytes
                .saturating_add(right.metadata_text_bytes),
        }
    }
}

impl std::ops::Sub for ProjectionUsage {
    type Output = Self;

    fn sub(self, right: Self) -> Self::Output {
        Self {
            dense_slots: self.dense_slots.saturating_sub(right.dense_slots),
            total_rows: self.total_rows.saturating_sub(right.total_rows),
            rich_entries: self.rich_entries.saturating_sub(right.rich_entries),
            layout_entries: self.layout_entries.saturating_sub(right.layout_entries),
            text_bytes: self.text_bytes.saturating_sub(right.text_bytes),
            metadata_text_bytes: self
                .metadata_text_bytes
                .saturating_sub(right.metadata_text_bytes),
        }
    }
}

fn projection_usage(file_data: &DocumentData) -> Result<ProjectionUsage, AppError> {
    let document_metadata = document_metadata_text_usage(file_data);
    ensure_limit(
        "metadata string bytes",
        document_metadata.maximum_string_bytes,
        MAX_METADATA_STRING_BYTES,
    )?;
    let mut usage = ProjectionUsage {
        dense_slots: 0,
        total_rows: 0,
        rich_entries: 0,
        layout_entries: 0,
        text_bytes: 0,
        metadata_text_bytes: file_data
            .path
            .len()
            .saturating_add(file_data.file_name.len()),
    };
    for sheet in &file_data.sheets {
        ensure_limit("sheet name bytes", sheet.name.len(), MAX_SHEET_NAME_BYTES)?;
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

fn sheet_resource_usage(sheet: &DocumentSheet) -> SheetResourceUsage {
    let dense_slots = sheet.rows.iter().map(Vec::len).sum();
    let text_bytes = sheet.rows.iter().flatten().map(cell_text_bytes).sum();
    let metadata_text_bytes = sheet_metadata_text_usage(sheet).total_bytes;
    let rich_entries = sheet.merges.len()
        + sheet.column_widths.as_ref().map_or(0, HashMap::len)
        + sheet.row_heights.as_ref().map_or(0, HashMap::len)
        + sheet.rich.cell_formats.len()
        + sheet.rich.cell_styles.len()
        + sheet.rich.hyperlinks.len()
        + sheet.rich.hidden_rows.len()
        + sheet.rich.hidden_columns.len()
        + sheet.rich.drawings.len()
        + sheet.rich.images.len();
    SheetResourceUsage {
        usage: ProjectionUsage {
            dense_slots,
            total_rows: sheet.rows.len(),
            rich_entries,
            layout_entries: sheet.column_widths.as_ref().map_or(0, HashMap::len)
                + sheet.row_heights.as_ref().map_or(0, HashMap::len),
            text_bytes,
            metadata_text_bytes,
        },
        extent: sheet.extent(),
    }
}

fn validate_sheet_metadata(
    sheet: &DocumentSheet,
    usage: &mut ProjectionUsage,
) -> Result<(), AppError> {
    let metadata = sheet_metadata_text_usage(sheet);
    ensure_limit(
        "metadata string bytes",
        metadata.maximum_string_bytes,
        MAX_METADATA_STRING_BYTES,
    )?;
    usage.metadata_text_bytes = checked_add(
        usage.metadata_text_bytes,
        metadata.total_bytes,
        "metadata text bytes",
    )?;
    for merge in &sheet.merges {
        validate_position(merge.start_row as usize, merge.start_col as usize)?;
        validate_position(merge.end_row as usize, merge.end_col as usize)?;
    }
    for (index, width) in sheet.column_widths.iter().flat_map(|values| values.iter()) {
        validate_position(0, *index)?;
        validate_column_width(Some(*width))?;
    }
    for (index, height) in sheet.row_heights.iter().flat_map(|values| values.iter()) {
        validate_position(*index, 0)?;
        validate_row_height(Some(*height))?;
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
    for image in &sheet.rich.images {
        let markers = match &image.anchor {
            crate::document_data::ImageAnchor::OneCell { from, .. } => (from, None),
            crate::document_data::ImageAnchor::TwoCell { from, to } => (from, Some(to)),
        };
        validate_position(markers.0.row as usize, markers.0.col as usize)?;
        if let Some(to) = markers.1 {
            validate_position(to.row as usize, to.col as usize)?;
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
        + sheet.rich.drawings.len()
        + sheet.rich.images.len();
    usage.rich_entries = checked_add(usage.rich_entries, rich_entries, "rich metadata entries")?;
    let layout_entries = sheet.column_widths.as_ref().map_or(0, HashMap::len)
        + sheet.row_heights.as_ref().map_or(0, HashMap::len);
    usage.layout_entries = checked_add(usage.layout_entries, layout_entries, "layout overrides")?;
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
    ensure_limit(
        "layout overrides",
        usage.layout_entries,
        MAX_LAYOUT_OVERRIDES,
    )?;
    ensure_limit("text bytes", usage.text_bytes, MAX_PROJECTED_TEXT_BYTES)?;
    ensure_limit(
        "metadata text bytes",
        usage.metadata_text_bytes,
        MAX_PROJECTED_METADATA_TEXT_BYTES,
    )
}

impl ProjectionUsage {
    fn estimated_bytes(self) -> usize {
        self.text_bytes
            .saturating_add(
                self.dense_slots
                    .saturating_mul(std::mem::size_of::<CellValue>()),
            )
            .saturating_add(
                self.total_rows
                    .saturating_mul(std::mem::size_of::<Vec<CellValue>>()),
            )
            .saturating_add(self.rich_entries.saturating_mul(96))
            .saturating_add(self.metadata_text_bytes)
    }
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
    use crate::domain::CellValue;

    #[test]
    fn rejects_remote_cell_targets_before_projection_growth() {
        let file_data = DocumentData {
            path: String::new(),
            file_name: "book.xlsx".to_string(),
            sheets: vec![DocumentSheet::default()],
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
        let file_data = DocumentData {
            path: String::new(),
            file_name: "book.xlsx".to_string(),
            sheets: vec![DocumentSheet {
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
            layout_entries: 0,
            text_bytes: MAX_PROJECTED_TEXT_BYTES + 1,
            metadata_text_bytes: 0,
        })
        .expect_err("projected text budget");

        assert!(matches!(error, AppError::ResourceLimitExceeded(_)));

        let metadata_error = validate_usage(ProjectionUsage {
            metadata_text_bytes: MAX_PROJECTED_METADATA_TEXT_BYTES + 1,
            ..ProjectionUsage::default()
        })
        .expect_err("projected metadata text budget");

        assert!(matches!(metadata_error, AppError::ResourceLimitExceeded(_)));
    }

    #[test]
    fn rejects_oversized_metadata_strings_even_with_few_entries() {
        let mut file_data = DocumentData {
            path: String::new(),
            file_name: "book.xlsx".to_string(),
            sheets: vec![DocumentSheet::default()],
        };
        file_data.sheets[0].rich.hyperlinks.insert(
            "A1".to_string(),
            crate::document_data::Hyperlink {
                url: "x".repeat(MAX_METADATA_STRING_BYTES + 1),
                tooltip: None,
                location: false,
            },
        );

        let error = validate_file_data(&file_data).expect_err("oversized hyperlink");

        assert!(matches!(error, AppError::ResourceLimitExceeded(_)));
    }

    #[test]
    fn rejects_identity_and_sheet_names_outside_manifest_limits() {
        let mut file_data = DocumentData {
            path: "x".repeat(MAX_DOCUMENT_PATH_BYTES + 1),
            file_name: "book.xlsx".to_string(),
            sheets: vec![DocumentSheet::default()],
        };
        assert!(matches!(
            validate_file_data(&file_data),
            Err(AppError::ResourceLimitExceeded(_))
        ));

        file_data.path.clear();
        file_data.file_name = "x".repeat(MAX_FILE_NAME_BYTES + 1);
        assert!(matches!(
            validate_file_data(&file_data),
            Err(AppError::ResourceLimitExceeded(_))
        ));

        file_data.file_name = "book.xlsx".to_string();
        file_data.sheets[0].name = "x".repeat(MAX_SHEET_NAME_BYTES + 1);
        assert!(matches!(
            validate_file_data(&file_data),
            Err(AppError::ResourceLimitExceeded(_))
        ));
    }

    #[test]
    fn rejects_layout_dimensions_outside_the_document_policy() {
        let mut file_data = DocumentData {
            path: String::new(),
            file_name: "book.xlsx".to_string(),
            sheets: vec![DocumentSheet::default()],
        };
        file_data.sheets[0].column_widths = Some(HashMap::from([(0, 0)]));
        assert!(matches!(
            validate_file_data(&file_data),
            Err(AppError::ResourceLimitExceeded(_))
        ));

        file_data.sheets[0].column_widths = Some(HashMap::from([(0, MAX_COLUMN_WIDTH_PX + 1)]));
        assert!(matches!(
            validate_file_data(&file_data),
            Err(AppError::ResourceLimitExceeded(_))
        ));
    }

    #[test]
    fn resource_ledger_refreshes_only_the_changed_sheet_extent() {
        let mut file_data = DocumentData {
            path: String::new(),
            file_name: "book.xlsx".to_string(),
            sheets: vec![DocumentSheet::default(), DocumentSheet::default()],
        };
        let mut ledger = ResourceLedger::from_file_data(&file_data);
        file_data.sheets[1].rows = vec![vec![CellValue::String("value".to_string())]];

        ledger.refresh_sheets(&file_data, [1]);

        assert_eq!(ledger.sheet_extent(0), Some(SheetExtent::default()));
        assert_eq!(
            ledger.sheet_extent(1),
            Some(SheetExtent {
                row_count: 1,
                column_count: 1,
            })
        );
    }

    #[test]
    fn resource_ledger_refreshes_identity_bytes_without_rebuilding_sheets() {
        let mut file_data = DocumentData {
            path: String::new(),
            file_name: "book.xlsx".to_string(),
            sheets: vec![DocumentSheet::default()],
        };
        let mut ledger = ResourceLedger::from_file_data(&file_data);
        let before = ledger.estimated_bytes();

        file_data.path = "x".repeat(4_096);
        ledger.refresh_identity(&file_data);

        assert!(ledger.estimated_bytes() >= before + 4_096);
    }
}
