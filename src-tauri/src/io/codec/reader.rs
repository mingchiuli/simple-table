use std::collections::HashMap;
use std::io::Cursor;

use crate::error::AppError;
use crate::io::projection_mapper::ProjectionMapper;
use crate::types::{
    CellFormatProjection, CellStyleProjection, CellValue, DrawingKind, DrawingProjection, FileData,
    MergeRange, SheetData, SheetRichProjection,
};
use csv::ReaderBuilder;
use serde_json::Value;
use umya_spreadsheet::{Cell, Workbook, Worksheet, reader};

const DEFAULT_COLUMN_WIDTH_PX: u32 = 120;
const DEFAULT_ROW_HEIGHT_PX: u32 = 72;
const EXCEL_DEFAULT_COLUMN_WIDTH: f64 = 8.38;

pub struct ReadFileResult {
    pub file_data: FileData,
    pub workbook: Option<Workbook>,
}

/// 从已读取的文件字节解析 FileData，并在 Excel 格式下保留原始 umya Workbook。
pub fn read_file_with_workbook_from_bytes(
    extension: &str,
    bytes: Vec<u8>,
    path: String,
    file_name: String,
) -> Result<ReadFileResult, AppError> {
    let cursor = Cursor::new(bytes);

    match extension.to_lowercase().as_str() {
        "xlsx" => read_xlsx_from_bytes(cursor, path, file_name),
        "csv" => read_csv_from_bytes(cursor, path, file_name),
        _ => Err(AppError::UnsupportedFormat),
    }
}

fn read_xlsx_from_bytes(
    cursor: Cursor<Vec<u8>>,
    path: String,
    file_name: String,
) -> Result<ReadFileResult, AppError> {
    let workbook = read_workbook_from_reader(cursor)?;

    Ok(ReadFileResult {
        file_data: FileData {
            path,
            file_name,
            sheets: ProjectionMapper::sheets_from_workbook(&workbook),
        },
        workbook: Some(workbook),
    })
}

fn read_workbook_from_reader(cursor: Cursor<Vec<u8>>) -> Result<Workbook, AppError> {
    reader::xlsx::read_reader(cursor, true).map_err(|e| AppError::ReadError(e.to_string()))
}

pub(crate) fn read_worksheet(worksheet: &Worksheet) -> SheetData {
    let (highest_col, highest_row) = worksheet.highest_column_and_row();
    let mut rows = vec![vec![CellValue::Null; highest_col as usize]; highest_row as usize];

    for cell in worksheet.cells() {
        let row_idx = cell.coordinate().row_num().saturating_sub(1) as usize;
        let col_idx = cell.coordinate().col_num().saturating_sub(1) as usize;
        if row_idx >= rows.len() {
            rows.resize_with(row_idx + 1, Vec::new);
        }
        if col_idx >= rows[row_idx].len() {
            rows[row_idx].resize(col_idx + 1, CellValue::Null);
        }
        rows[row_idx][col_idx] = cell_to_value(cell);
    }

    SheetData {
        name: worksheet.name().to_string(),
        rows,
        merges: read_merge_ranges(worksheet),
        column_widths: read_column_widths(worksheet),
        row_heights: read_row_heights(worksheet),
        rich: read_rich_projection(worksheet),
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
            return CellValue::Number(Value::from(number as i64));
        }
        return CellValue::Number(Value::from(number));
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

fn read_rich_projection(worksheet: &Worksheet) -> SheetRichProjection {
    SheetRichProjection {
        cell_formats: read_cell_formats(worksheet),
        cell_styles: read_cell_styles(worksheet),
        drawings: read_drawings(worksheet),
        has_more_drawings: false,
    }
}

fn read_cell_formats(worksheet: &Worksheet) -> HashMap<String, CellFormatProjection> {
    worksheet
        .cells()
        .into_iter()
        .filter_map(|cell| {
            let style = cell.style();
            let number_format = style
                .numbering_format()
                .map(|format| format.format_code().to_string())
                .filter(|value| !value.is_empty());
            let projection = CellFormatProjection {
                number_format,
                style_id: None,
            };
            has_format_projection(&projection).then(|| (cell.coordinate().to_string(), projection))
        })
        .collect()
}

fn has_format_projection(format: &CellFormatProjection) -> bool {
    format.number_format.is_some() || format.style_id.is_some()
}

fn read_cell_styles(worksheet: &Worksheet) -> HashMap<String, CellStyleProjection> {
    worksheet
        .cells()
        .into_iter()
        .filter_map(|cell| {
            let style = cell.style();
            let font = style.font();
            let projection = CellStyleProjection {
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

fn has_style_projection(style: &CellStyleProjection) -> bool {
    style.font_color.is_some()
        || style.background_color.is_some()
        || style.bold.is_some()
        || style.italic.is_some()
        || style.horizontal_align.is_some()
        || style.vertical_align.is_some()
        || style.number_format.is_some()
}

fn read_drawings(worksheet: &Worksheet) -> Vec<DrawingProjection> {
    let mut drawings = Vec::new();
    drawings.extend(worksheet.image_collection().iter().map(|image| {
        let from = image.from_marker_type();
        let to = image.to_marker_type();
        DrawingProjection {
            kind: DrawingKind::Image,
            from_row: from.row(),
            from_col: from.col(),
            to_row: to.map(|marker| marker.row()),
            to_col: to.map(|marker| marker.col()),
        }
    }));
    drawings.extend(worksheet.chart_collection().iter().filter_map(|chart| {
        let (col, row) = parse_coordinate_1_based(&chart.coordinate())?;
        Some(DrawingProjection {
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

fn excel_column_width_to_px(width: f64) -> u32 {
    if width <= 0.0 {
        return DEFAULT_COLUMN_WIDTH_PX;
    }
    ((width * 7.0) + 5.0).round().max(1.0) as u32
}

fn is_default_column_width(width: f64, px: u32) -> bool {
    px == DEFAULT_COLUMN_WIDTH_PX || (width - EXCEL_DEFAULT_COLUMN_WIDTH).abs() < 0.001
}

fn points_to_px(points: f64) -> u32 {
    if points <= 0.0 {
        return DEFAULT_ROW_HEIGHT_PX;
    }
    (points * 96.0 / 72.0).round().max(1.0) as u32
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

    for result in reader.records() {
        let record = result.map_err(|e| AppError::ReadError(e.to_string()))?;
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
                    CellValue::Number(Value::from(int_val))
                } else if let Ok(num) = field.parse::<f64>() {
                    if num.is_finite() {
                        CellValue::Number(Value::from(num))
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

    Ok(ReadFileResult {
        file_data: FileData {
            path,
            file_name,
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows,
                ..Default::default()
            }],
        },
        workbook: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn column_width_conversion_is_stable_for_ui_default() {
        assert_eq!(excel_column_width_to_px(16.428571428571427), 120);
    }

    #[test]
    fn default_umya_column_width_is_not_persisted_as_custom_layout() {
        assert!(is_default_column_width(
            8.38,
            excel_column_width_to_px(8.38)
        ));
    }

    #[test]
    fn row_height_conversion_uses_pixels() {
        assert_eq!(points_to_px(54.0), 72);
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
}
