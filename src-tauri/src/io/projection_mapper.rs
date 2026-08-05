use crate::document_data::{
    CellFormat, CellStyle, DocumentData, DocumentSheet, Drawing, DrawingKind, FreezePane,
    Hyperlink, ImageAnchor, ImageMarker, MergeRange, RichMetadata, SheetImage,
};
use crate::document_layout_policy::DEFAULT_ROW_HEIGHT_PX;
use crate::domain::{CellNumber, CellValue};
use crate::error::AppError;
use crate::io::codec::address::coordinate;
use crate::io::layout_units::{excel_column_width_to_px, is_default_column_width, points_to_px};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use umya_spreadsheet::{Cell, Workbook, Worksheet};

pub(crate) struct ProjectionMapper;

impl ProjectionMapper {
    pub(crate) fn sheets_from_workbook(workbook: &Workbook) -> Vec<DocumentSheet> {
        workbook
            .sheet_collection()
            .iter()
            .map(read_worksheet)
            .collect()
    }

    pub(crate) fn refresh_file_data_from_workbook(
        workbook: &Workbook,
        file_data: &mut DocumentData,
    ) {
        file_data.sheets = Self::sheets_from_workbook(workbook);
    }

    pub(crate) fn sync_merge_ranges_to_workbook(
        workbook: &mut Workbook,
        file_data: &DocumentData,
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
        projection: &DocumentData,
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

    pub(crate) fn validate_workbook_sheets_match_projection(
        workbook: &Workbook,
        projection: &DocumentData,
        sheet_indexes: impl IntoIterator<Item = usize>,
    ) -> Result<(), AppError> {
        if workbook.sheet_count() != projection.sheets.len() {
            return Err(AppError::Internal(format!(
                "workbook/projection sheet count mismatch: workbook={}, projection={}",
                workbook.sheet_count(),
                projection.sheets.len()
            )));
        }

        for sheet_index in sheet_indexes {
            let Some(worksheet) = workbook.sheet_collection().get(sheet_index) else {
                return Err(AppError::Internal(format!(
                    "workbook is missing sheet {sheet_index}"
                )));
            };
            let Some(expected) = projection.sheets.get(sheet_index) else {
                return Err(AppError::Internal(format!(
                    "projection is missing sheet {sheet_index}"
                )));
            };
            let actual = read_worksheet(worksheet);
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

fn read_worksheet(worksheet: &Worksheet) -> DocumentSheet {
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
    let images = read_images(worksheet);
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
        images,
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

fn read_images(worksheet: &Worksheet) -> Vec<SheetImage> {
    worksheet
        .image_collection()
        .iter()
        .enumerate()
        .map(|(z_index, image)| {
            let bytes = image.image_data();
            let media_id = Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let mime_type = image_mime_type(image.image_name(), bytes).to_string();
            let (intrinsic_width, intrinsic_height) =
                image::ImageReader::new(std::io::Cursor::new(bytes))
                    .with_guessed_format()
                    .ok()
                    .and_then(|reader| reader.into_dimensions().ok())
                    .unwrap_or((0, 0));
            let renderable = matches!(mime_type.as_str(), "image/png" | "image/jpeg")
                && !bytes.is_empty()
                && bytes.len() <= crate::document_data::MAX_EMBEDDED_IMAGE_BYTES
                && intrinsic_width > 0
                && intrinsic_height > 0
                && u64::from(intrinsic_width) * u64::from(intrinsic_height)
                    <= crate::document_data::MAX_RENDER_IMAGE_PIXELS;
            SheetImage {
                id: image_session_id(image, z_index, &media_id),
                media_id,
                mime_type: mime_type.clone(),
                intrinsic_width,
                intrinsic_height,
                anchor: image_anchor(image),
                z_index,
                renderable,
            }
        })
        .collect()
}

fn image_anchor(image: &umya_spreadsheet::Image) -> ImageAnchor {
    if let Some(anchor) = image.two_cell_anchor() {
        return ImageAnchor::TwoCell {
            from: image_marker(anchor.from_marker()),
            to: image_marker(anchor.to_marker()),
        };
    }
    if let Some(anchor) = image.one_cell_anchor() {
        return ImageAnchor::OneCell {
            from: image_marker(anchor.from_marker()),
            width_emu: anchor.extent().cx(),
            height_emu: anchor.extent().cy(),
        };
    }
    ImageAnchor::OneCell {
        from: ImageMarker::default(),
        width_emu: 0,
        height_emu: 0,
    }
}

fn image_marker(
    marker: &umya_spreadsheet::structs::drawing::spreadsheet::MarkerType,
) -> ImageMarker {
    ImageMarker {
        row: marker.row(),
        col: marker.col(),
        row_offset_emu: marker.row_off(),
        col_offset_emu: marker.col_off(),
    }
}

fn image_session_id(image: &umya_spreadsheet::Image, z_index: usize, media_id: &str) -> String {
    let picture = image
        .one_cell_anchor()
        .and_then(|anchor| anchor.picture())
        .or_else(|| image.two_cell_anchor().and_then(|anchor| anchor.picture()));
    let properties = picture.map(|picture| {
        picture
            .non_visual_picture_properties()
            .non_visual_drawing_properties()
    });
    if let Some(name) = properties.map(|properties| properties.name())
        && let Some(id) = name.strip_prefix("simple-table-image-")
    {
        return id.to_string();
    }
    let drawing_id = properties
        .map(|properties| properties.id())
        .unwrap_or_default();
    format!(
        "xlsx-{drawing_id}-{z_index}-{}",
        &media_id[..media_id.len().min(12)]
    )
}

fn image_mime_type(name: &str, bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return "image/png";
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return "image/jpeg";
    }
    match std::path::Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        Some("tif" | "tiff") => "image/tiff",
        Some("emf") => "image/emf",
        _ => "application/octet-stream",
    }
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

fn sheet_difference(expected: &DocumentSheet, actual: &DocumentSheet) -> String {
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

fn sheets_are_consistent(expected: &DocumentSheet, actual: &DocumentSheet) -> bool {
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
            (expected.as_f64() - actual.as_f64()).abs() < 0.000_000_1
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

#[cfg(test)]
mod tests {
    use super::*;

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
