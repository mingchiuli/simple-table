use crate::document_data::{
    CellFormat, CellStyle, DocumentData, DocumentSheet, Drawing, DrawingKind, FreezePane,
    Hyperlink, MergeRange, RichMetadata,
};
use std::collections::HashMap;
use std::io::Cursor;

use crate::document_format::SpreadsheetFileFormat;
use crate::document_layout_policy::DEFAULT_ROW_HEIGHT_PX;
use crate::domain::{CellNumber, CellValue};
use crate::error::AppError;
use crate::io::input_limits::{
    MAX_XLSX_ARCHIVE_ENTRIES, MAX_XLSX_UNCOMPRESSED_BYTES, validate_input_size,
};
use crate::io::layout_units::{excel_column_width_to_px, is_default_column_width, points_to_px};
use crate::io::projection_codec::WorkbookProjectionCodec;
use crate::resource_limits::{
    MAX_DENSE_CELL_SLOTS, MAX_ROWS_PER_SHEET, MAX_TOTAL_ROWS, MAX_WORKBOOK_SHEETS,
    validate_file_data, validate_position,
};
use csv::ReaderBuilder;
use umya_spreadsheet::{Cell, Workbook, Worksheet, reader};

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
        sheets: WorkbookProjectionCodec::read_sheets(&workbook),
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

pub(crate) fn read_worksheet(worksheet: &Worksheet) -> DocumentSheet {
    let mut rows: Vec<Vec<CellValue>> = Vec::new();

    for cell in worksheet.cells() {
        let value = cell_to_value(cell);
        if matches!(value, CellValue::Null) {
            continue;
        }

        let row_idx = cell.coordinate().row_num().saturating_sub(1) as usize;
        let col_idx = cell.coordinate().col_num().saturating_sub(1) as usize;
        if row_idx >= rows.len() {
            rows.resize_with(row_idx + 1, Vec::new);
        }
        if col_idx >= rows[row_idx].len() {
            rows[row_idx].resize(col_idx + 1, CellValue::Null);
        }
        rows[row_idx][col_idx] = value;
    }
    trim_trailing_empty_projection(&mut rows);

    DocumentSheet {
        name: worksheet.name().to_string(),
        rows,
        merges: read_merge_ranges(worksheet),
        column_widths: read_column_widths(worksheet),
        row_heights: read_row_heights(worksheet),
        rich: read_rich_projection(worksheet),
    }
}

fn trim_trailing_empty_projection(rows: &mut Vec<Vec<CellValue>>) {
    for row in rows.iter_mut() {
        while row
            .last()
            .is_some_and(|cell| matches!(cell, CellValue::Null))
        {
            row.pop();
        }
    }
    while rows.last().is_some_and(Vec::is_empty) {
        rows.pop();
    }
}

fn cell_to_value(cell: &Cell) -> CellValue {
    let cell_value = cell.cell_value();
    let cached_value = raw_cell_value(cell);
    if cell_value.is_formula() {
        return CellValue::formula(cell_value.formula().to_string(), cached_value);
    }
    cached_value
}

fn raw_cell_value(cell: &Cell) -> CellValue {
    if cell.data_type() == "b" {
        return CellValue::Boolean(matches!(
            cell.value().as_ref().to_ascii_lowercase().as_str(),
            "1" | "true"
        ));
    }

    if cell.data_type() == "e" {
        return CellValue::String(cell.value().into_owned());
    }

    if let Some(number) = cell.value_number()
        && number.is_finite()
    {
        if number.fract() == 0.0 && number >= i64::MIN as f64 && number <= i64::MAX as f64 {
            return CellValue::Number(CellNumber::from(number as i64));
        }
        return CellValue::Number(CellNumber::from_f64(number).expect("finite workbook number"));
    }

    let value = cell.value().into_owned();
    if value.is_empty() {
        CellValue::Null
    } else {
        CellValue::String(value)
    }
}

fn read_merge_ranges(worksheet: &Worksheet) -> Vec<MergeRange> {
    worksheet
        .merge_cells()
        .iter()
        .filter_map(|range| {
            let start_row = range.coordinate_start_row()?.num().checked_sub(1)?;
            let start_col = range.coordinate_start_col()?.num().checked_sub(1)?;
            let end_row = range
                .coordinate_end_row()
                .map(|row| row.num())
                .unwrap_or(start_row + 1)
                .checked_sub(1)?;
            let end_col = range
                .coordinate_end_col()
                .map(|col| col.num())
                .unwrap_or(start_col + 1)
                .checked_sub(1)?;
            Some(MergeRange {
                start_row,
                start_col: start_col as u16,
                end_row,
                end_col: end_col as u16,
            })
        })
        .collect()
}

fn read_column_widths(worksheet: &Worksheet) -> Option<HashMap<usize, u32>> {
    let widths: HashMap<usize, u32> = worksheet
        .column_dimensions()
        .iter()
        .filter_map(|column| {
            let px = excel_column_width_to_px(column.width());
            if is_default_column_width(column.width(), px) {
                None
            } else {
                Some((column.col_num().saturating_sub(1) as usize, px))
            }
        })
        .collect();
    (!widths.is_empty()).then_some(widths)
}

fn read_row_heights(worksheet: &Worksheet) -> Option<HashMap<usize, u32>> {
    let heights: HashMap<usize, u32> = worksheet
        .row_dimensions()
        .into_iter()
        .filter_map(|row| {
            let px = points_to_px(row.height());
            if px == DEFAULT_ROW_HEIGHT_PX {
                None
            } else {
                Some((row.row_num().saturating_sub(1) as usize, px))
            }
        })
        .collect();
    (!heights.is_empty()).then_some(heights)
}

fn read_rich_projection(worksheet: &Worksheet) -> RichMetadata {
    let cell_formats = read_cell_formats(worksheet);
    let cell_styles = read_cell_styles(worksheet);
    let freeze_pane = read_freeze_pane(worksheet);
    let hyperlinks = read_hyperlinks(worksheet);
    let drawings = read_drawings(worksheet);
    let has_style_metadata = !cell_formats.is_empty() || !cell_styles.is_empty();
    let has_hyperlinks = !hyperlinks.is_empty();
    let has_freeze_pane = freeze_pane.is_some();

    RichMetadata {
        cell_formats,
        cell_styles,
        hidden_rows: read_hidden_rows(worksheet),
        hidden_columns: read_hidden_columns(worksheet),
        freeze_pane,
        hyperlinks,
        drawings,
        has_more_drawings: false,
        has_style_metadata,
        has_hyperlinks,
        has_freeze_pane,
    }
}

fn read_hidden_rows(worksheet: &Worksheet) -> Vec<usize> {
    worksheet
        .row_dimensions()
        .into_iter()
        .filter_map(|row| {
            row.hidden()
                .then_some(row.row_num().saturating_sub(1) as usize)
        })
        .collect()
}

fn read_hidden_columns(worksheet: &Worksheet) -> Vec<usize> {
    worksheet
        .column_dimensions()
        .iter()
        .filter_map(|column| {
            column
                .hidden()
                .then_some(column.col_num().saturating_sub(1) as usize)
        })
        .collect()
}

fn read_freeze_pane(worksheet: &Worksheet) -> Option<FreezePane> {
    worksheet
        .sheets_views()
        .sheet_view_list()
        .iter()
        .find_map(|sheet_view| sheet_view.pane())
        .map(|pane| FreezePane {
            top_left_cell: pane.top_left_cell().to_string(),
            horizontal_split: pane.horizontal_split(),
            vertical_split: pane.vertical_split(),
            active_pane: format!("{:?}", pane.active_pane()),
            state: format!("{:?}", pane.state()),
        })
}

fn read_hyperlinks(worksheet: &Worksheet) -> HashMap<String, Hyperlink> {
    worksheet
        .cells()
        .into_iter()
        .filter_map(|cell| {
            let hyperlink = cell.hyperlink()?;
            Some((
                cell.coordinate().to_string(),
                Hyperlink {
                    url: hyperlink.url().to_string(),
                    tooltip: (!hyperlink.tooltip().is_empty())
                        .then(|| hyperlink.tooltip().to_string()),
                    location: hyperlink.location(),
                },
            ))
        })
        .collect()
}

fn read_cell_formats(worksheet: &Worksheet) -> HashMap<String, CellFormat> {
    worksheet
        .cells()
        .into_iter()
        .filter_map(|cell| {
            let style = cell.style();
            let number_format = style
                .numbering_format()
                .map(|format| format.format_code().to_string())
                .filter(|value| !value.is_empty());
            let projection = CellFormat {
                number_format,
                style_id: None,
            };
            has_format_projection(&projection).then(|| (cell.coordinate().to_string(), projection))
        })
        .collect()
}

fn has_format_projection(format: &CellFormat) -> bool {
    format.number_format.is_some() || format.style_id.is_some()
}

fn read_cell_styles(worksheet: &Worksheet) -> HashMap<String, CellStyle> {
    worksheet
        .cells()
        .into_iter()
        .filter_map(|cell| {
            let style = cell.style();
            let font = style.font();
            let projection = CellStyle {
                font_color: font
                    .map(|font| font.color().argb_str())
                    .filter(|value| !value.is_empty()),
                background_color: style
                    .background_color()
                    .map(|color| color.argb_str())
                    .filter(|value| !value.is_empty()),
                bold: font.map(|font| font.bold()).filter(|value| *value),
                italic: font.map(|font| font.italic()).filter(|value| *value),
                horizontal_align: style
                    .alignment()
                    .map(|alignment| format!("{:?}", alignment.horizontal()))
                    .filter(|value| !value.is_empty()),
                vertical_align: style
                    .alignment()
                    .map(|alignment| format!("{:?}", alignment.vertical()))
                    .filter(|value| !value.is_empty()),
                number_format: style
                    .numbering_format()
                    .map(|format| format.format_code().to_string())
                    .filter(|value| !value.is_empty()),
            };
            has_style_projection(&projection).then(|| (cell.coordinate().to_string(), projection))
        })
        .collect()
}

fn has_style_projection(style: &CellStyle) -> bool {
    style.font_color.is_some()
        || style.background_color.is_some()
        || style.bold.is_some()
        || style.italic.is_some()
        || style.horizontal_align.is_some()
        || style.vertical_align.is_some()
        || style.number_format.is_some()
}

fn read_drawings(worksheet: &Worksheet) -> Vec<Drawing> {
    let mut drawings = Vec::new();
    drawings.extend(worksheet.image_collection().iter().map(|image| {
        let from = image.from_marker_type();
        let to = image.to_marker_type();
        Drawing {
            kind: DrawingKind::Image,
            from_row: from.row(),
            from_col: from.col(),
            to_row: to.map(|marker| marker.row()),
            to_col: to.map(|marker| marker.col()),
        }
    }));
    drawings.extend(worksheet.chart_collection().iter().filter_map(|chart| {
        let (col, row) = parse_coordinate_1_based(&chart.coordinate())?;
        Some(Drawing {
            kind: DrawingKind::Chart,
            from_row: row.saturating_sub(1),
            from_col: col.saturating_sub(1),
            to_row: None,
            to_col: None,
        })
    }));
    drawings
}

fn parse_coordinate_1_based(coordinate: &str) -> Option<(u32, u32)> {
    let mut col = 0u32;
    let mut row = 0u32;
    for byte in coordinate.bytes() {
        if byte.is_ascii_alphabetic() {
            col = col
                .saturating_mul(26)
                .saturating_add(u32::from(byte.to_ascii_uppercase() - b'A' + 1));
        } else if byte.is_ascii_digit() {
            row = row
                .saturating_mul(10)
                .saturating_add(u32::from(byte - b'0'));
        }
    }
    (col > 0 && row > 0).then_some((col, row))
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

    #[test]
    fn rich_projection_includes_cell_format_and_style_metadata() {
        let mut workbook = umya_spreadsheet::new_file();
        let sheet = workbook.sheet_mut(0).expect("sheet");
        sheet.cell_mut("A1").set_value_string("styled");
        sheet.cell_mut("A1").style_mut().font_mut().set_bold(true);
        sheet
            .cell_mut("A1")
            .style_mut()
            .numbering_format_mut()
            .set_format_code("0.00");

        let data = read_worksheet(sheet);

        assert_eq!(
            data.rich
                .cell_formats
                .get("A1")
                .and_then(|format| format.number_format.as_deref()),
            Some("0.00")
        );
        assert_eq!(
            data.rich.cell_styles.get("A1").and_then(|style| style.bold),
            Some(true)
        );
    }

    #[test]
    fn rich_projection_includes_hidden_layout_freeze_pane_and_hyperlinks() {
        let mut workbook = umya_spreadsheet::new_file();
        let sheet = workbook.sheet_mut(0).expect("sheet");
        sheet.row_dimension_mut(2).set_hidden(true);
        sheet.column_dimension_mut("B").set_hidden(true);
        sheet
            .cell_mut("C3")
            .hyperlink_mut()
            .set_url("https://example.com")
            .set_tooltip("Example");

        let mut pane = umya_spreadsheet::Pane::default();
        pane.set_horizontal_split(2.0)
            .set_vertical_split(3.0)
            .set_active_pane(umya_spreadsheet::PaneValues::BottomRight)
            .set_state(umya_spreadsheet::PaneStateValues::Frozen);
        pane.top_left_cell_mut().set_coordinate("C4");
        sheet
            .sheet_views_mut()
            .sheet_view_list_mut()
            .first_mut()
            .expect("sheet view")
            .set_pane(pane);

        let data = read_worksheet(sheet);

        assert_eq!(data.rich.hidden_rows, vec![1]);
        assert_eq!(data.rich.hidden_columns, vec![1]);
        assert_eq!(
            data.rich.hyperlinks.get("C3").map(|hyperlink| (
                hyperlink.url.as_str(),
                hyperlink.tooltip.as_deref(),
                hyperlink.location
            )),
            Some(("https://example.com", Some("Example"), false))
        );
        assert_eq!(
            data.rich.freeze_pane.as_ref().map(|pane| (
                pane.top_left_cell.as_str(),
                pane.horizontal_split,
                pane.vertical_split,
                pane.state.as_str()
            )),
            Some(("C4", 2.0, 3.0, "Frozen"))
        );
    }

    #[test]
    fn style_only_far_cells_do_not_expand_dense_rows() {
        let mut workbook = umya_spreadsheet::new_file();
        let sheet = workbook.sheet_mut(0).expect("sheet");
        sheet.cell_mut("A1").set_value_string("value");
        sheet
            .cell_mut("Z1000")
            .style_mut()
            .font_mut()
            .set_bold(true);

        let data = read_worksheet(sheet);

        assert_eq!(data.rows.len(), 1);
        assert_eq!(data.rows[0].len(), 1);
        assert_eq!(
            data.rich
                .cell_styles
                .get("Z1000")
                .and_then(|style| style.bold),
            Some(true)
        );
    }
}
