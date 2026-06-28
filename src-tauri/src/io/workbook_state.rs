use crate::error::AppError;
use crate::io::codec::reader::read_worksheet;
use crate::io::codec::writer::{
    px_to_excel_column_width, px_to_points, sync_sheet_from_sheet_data, write_cell,
};
use crate::ops::Operation;
use crate::types::{FileData, OperationResult, SheetCellChange, SheetData};
use regex::{Captures, Regex};
use umya_spreadsheet::{Workbook, Worksheet};

#[derive(Clone, Copy)]
enum StructureShift {
    InsertRows { row_index: usize, count: usize },
    DeleteRows { row_index: usize, count: usize },
    InsertColumns { col_index: usize, count: usize },
    DeleteColumns { col_index: usize, count: usize },
}

pub fn patch_after_operation(
    workbook: &mut Workbook,
    file_data: &mut FileData,
    operation: &Operation,
    result: &OperationResult,
    cell_changes: &[SheetCellChange],
) -> Result<(), AppError> {
    match operation {
        Operation::SetCell {
            sheet_index,
            row,
            col,
            ..
        } => {
            patch_cell(workbook, file_data, *sheet_index, *row, *col)?;
            patch_cell_changes(workbook, file_data, cell_changes)?;
        }
        Operation::AddRow {
            sheet_index,
            row_index,
            row_data,
            row_height,
            ..
        } => {
            let sheet_name = sheet_name(workbook, *sheet_index)?;
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                worksheet.insert_new_row(*row_index as u32 + 1, 1);
                patch_row_cells(worksheet, *row_index, row_data);
                if let Some(height) = row_height {
                    patch_row_height(worksheet, *row_index, Some(*height));
                }
            }
            adjust_workbook_formulas(
                workbook,
                &sheet_name,
                StructureShift::InsertRows {
                    row_index: *row_index,
                    count: 1,
                },
            );
            refresh_projection(workbook, file_data);
            patch_cell_changes(workbook, file_data, cell_changes)?;
        }
        Operation::DeleteRow {
            sheet_index,
            row_index,
            ..
        } => {
            let sheet_name = sheet_name(workbook, *sheet_index)?;
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                worksheet.remove_row(*row_index as u32 + 1, 1);
            }
            adjust_workbook_formulas(
                workbook,
                &sheet_name,
                StructureShift::DeleteRows {
                    row_index: *row_index,
                    count: 1,
                },
            );
            refresh_projection(workbook, file_data);
            patch_cell_changes(workbook, file_data, cell_changes)?;
        }
        Operation::AddColumn {
            sheet_index,
            col_index,
            col_data,
            column_width,
            ..
        } => {
            let actual_col_index = match result {
                OperationResult::AddColumn { column, .. } => column.index,
                _ => col_index.unwrap_or_else(|| {
                    file_data
                        .sheets
                        .get(*sheet_index)
                        .and_then(|sheet| sheet.rows.first())
                        .map(|row| row.len().saturating_sub(1))
                        .unwrap_or(0)
                }),
            };
            let sheet_name = sheet_name(workbook, *sheet_index)?;
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                worksheet.insert_new_column_by_index(actual_col_index as u32 + 1, 1);
                patch_column_cells(worksheet, actual_col_index, col_data);
                if let Some(width) = column_width {
                    patch_column_width(worksheet, actual_col_index, Some(*width));
                }
            }
            adjust_workbook_formulas(
                workbook,
                &sheet_name,
                StructureShift::InsertColumns {
                    col_index: actual_col_index,
                    count: 1,
                },
            );
            refresh_projection(workbook, file_data);
            patch_cell_changes(workbook, file_data, cell_changes)?;
        }
        Operation::DeleteColumn {
            sheet_index,
            col_index,
            ..
        } => {
            let sheet_name = sheet_name(workbook, *sheet_index)?;
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                worksheet.remove_column_by_index(*col_index as u32 + 1, 1);
            }
            adjust_workbook_formulas(
                workbook,
                &sheet_name,
                StructureShift::DeleteColumns {
                    col_index: *col_index,
                    count: 1,
                },
            );
            refresh_projection(workbook, file_data);
            patch_cell_changes(workbook, file_data, cell_changes)?;
        }
        Operation::SetColumnWidth {
            sheet_index,
            col_index,
            new_width,
            ..
        } => {
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                patch_column_width(worksheet, *col_index, *new_width);
            }
        }
        Operation::SetRowHeight {
            sheet_index,
            row_index,
            new_height,
            ..
        } => {
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                patch_row_height(worksheet, *row_index, *new_height);
            }
        }
        Operation::AddSheet { .. } => {
            if let OperationResult::AddSheet {
                sheet_index,
                sheet_data,
                ..
            } = result
            {
                insert_sheet(workbook, *sheet_index, sheet_data)?;
                refresh_projection(workbook, file_data);
                patch_cell_changes(workbook, file_data, cell_changes)?;
            }
        }
        Operation::DeleteSheet { .. } => {
            if let OperationResult::DeleteSheet { sheet_index, .. } = result {
                remove_sheet(workbook, *sheet_index)?;
                refresh_projection(workbook, file_data);
                patch_cell_changes(workbook, file_data, cell_changes)?;
            }
        }
    }

    Ok(())
}

pub fn patch_formula_changes(
    workbook: &mut Workbook,
    file_data: &mut FileData,
    cell_changes: &[SheetCellChange],
) -> Result<(), AppError> {
    patch_cell_changes(workbook, file_data, cell_changes)
}

fn patch_cell_changes(
    workbook: &mut Workbook,
    file_data: &mut FileData,
    cell_changes: &[SheetCellChange],
) -> Result<(), AppError> {
    for change in cell_changes {
        patch_cell(
            workbook,
            file_data,
            change.sheet_index,
            change.row,
            change.col,
        )?;
    }
    Ok(())
}

fn patch_row_cells(
    worksheet: &mut Worksheet,
    row_index: usize,
    row_data: &[crate::types::CellValue],
) {
    for (col_index, cell) in row_data.iter().enumerate() {
        write_cell(worksheet, row_index as u32 + 1, col_index as u32 + 1, cell);
    }
}

fn patch_column_cells(
    worksheet: &mut Worksheet,
    col_index: usize,
    col_data: &[crate::types::CellValue],
) {
    for (row_index, cell) in col_data.iter().enumerate() {
        write_cell(worksheet, row_index as u32 + 1, col_index as u32 + 1, cell);
    }
}

fn adjust_workbook_formulas(
    workbook: &mut Workbook,
    target_sheet_name: &str,
    shift: StructureShift,
) {
    for worksheet in workbook.sheet_collection_mut() {
        let current_sheet_name = worksheet.name().to_string();
        if current_sheet_name == target_sheet_name {
            continue;
        }
        for cell in worksheet.cells_mut() {
            if !cell.is_formula() {
                continue;
            }
            let adjusted = adjust_formula_references(
                cell.formula(),
                target_sheet_name,
                &current_sheet_name,
                shift,
            );
            if adjusted != cell.formula() {
                cell.set_formula(adjusted);
            }
        }
    }
}

fn adjust_formula_references(
    formula: &str,
    target_sheet_name: &str,
    current_sheet_name: &str,
    shift: StructureShift,
) -> String {
    let mut adjusted = String::with_capacity(formula.len());
    let bytes = formula.as_bytes();
    let mut segment_start = 0;
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] != b'"' {
            index += 1;
            continue;
        }

        if segment_start < index {
            adjusted.push_str(&adjust_formula_reference_segment(
                &formula[segment_start..index],
                target_sheet_name,
                current_sheet_name,
                shift,
            ));
        }

        let string_start = index;
        index += 1;
        while index < bytes.len() {
            if bytes[index] == b'"' {
                if index + 1 < bytes.len() && bytes[index + 1] == b'"' {
                    index += 2;
                } else {
                    index += 1;
                    break;
                }
            } else {
                index += 1;
            }
        }
        adjusted.push_str(&formula[string_start..index]);
        segment_start = index;
    }

    if segment_start < formula.len() {
        adjusted.push_str(&adjust_formula_reference_segment(
            &formula[segment_start..],
            target_sheet_name,
            current_sheet_name,
            shift,
        ));
    }

    adjusted
}

fn adjust_formula_reference_segment(
    formula: &str,
    target_sheet_name: &str,
    current_sheet_name: &str,
    shift: StructureShift,
) -> String {
    let re = Regex::new(
        r#"(?x)
        (?P<prefix>(?:'(?P<quoted>(?:[^']|'')+)'|(?P<sheet>[A-Za-z_][A-Za-z0-9_ .]*))!)?
        (?P<start_col>\$?[A-Z]{1,3})(?P<start_row>\$?\d+)
        (?:
            :
            (?P<end_col>\$?[A-Z]{1,3})(?P<end_row>\$?\d+)
        )?
        "#,
    )
    .expect("valid formula reference regex");

    let mut adjusted = String::with_capacity(formula.len());
    let mut last_end = 0;

    for captures in re.captures_iter(formula) {
        let Some(matched) = captures.get(0) else {
            continue;
        };

        adjusted.push_str(&formula[last_end..matched.start()]);
        last_end = matched.end();

        if !is_reference_match_boundary(formula, matched.start(), matched.end()) {
            adjusted.push_str(matched.as_str());
            continue;
        }

        let sheet_name = captures
            .name("quoted")
            .map(|m| m.as_str().replace("''", "'"))
            .or_else(|| captures.name("sheet").map(|m| m.as_str().to_string()));

        let applies_to_reference = sheet_name
            .as_deref()
            .map(|name| name == target_sheet_name)
            .unwrap_or(current_sheet_name == target_sheet_name);
        if !applies_to_reference {
            adjusted.push_str(matched.as_str());
            continue;
        }

        adjusted.push_str(&adjust_reference_match(&captures, shift));
    }

    adjusted.push_str(&formula[last_end..]);
    adjusted
}

fn is_reference_match_boundary(formula: &str, start: usize, end: usize) -> bool {
    let previous = formula[..start].chars().next_back();
    let next = formula[end..].chars().next();

    !previous.is_some_and(is_reference_identifier_char)
        && !next.is_some_and(is_reference_identifier_char)
}

fn is_reference_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '.'
}

fn adjust_reference_match(captures: &Captures<'_>, shift: StructureShift) -> String {
    let prefix = captures.name("prefix").map(|m| m.as_str()).unwrap_or("");
    let start_col = &captures["start_col"];
    let start_row = &captures["start_row"];

    match (captures.name("end_col"), captures.name("end_row")) {
        (Some(end_col), Some(end_row)) => {
            match adjust_range_reference(
                start_col,
                start_row,
                end_col.as_str(),
                end_row.as_str(),
                shift,
            ) {
                Some((start, end)) => format!("{prefix}{start}:{end}"),
                None => format!("{prefix}#REF!"),
            }
        }
        _ => match adjust_cell_reference(start_col, start_row, shift) {
            Some(start) => format!("{prefix}{start}"),
            None => format!("{prefix}#REF!"),
        },
    }
}

#[derive(Clone, Copy)]
struct CellReference {
    col_index: usize,
    row_index: usize,
    col_locked: bool,
    row_locked: bool,
}

fn parse_cell_reference(col: &str, row: &str) -> Option<CellReference> {
    Some(CellReference {
        col_locked: col.starts_with('$'),
        row_locked: row.starts_with('$'),
        col_index: column_label_to_index(col.trim_start_matches('$'))?,
        row_index: row
            .trim_start_matches('$')
            .parse::<usize>()
            .ok()?
            .checked_sub(1)?,
    })
}

fn format_cell_reference(cell_ref: CellReference) -> String {
    format!(
        "{}{}{}{}",
        if cell_ref.col_locked { "$" } else { "" },
        column_index_to_label(cell_ref.col_index),
        if cell_ref.row_locked { "$" } else { "" },
        cell_ref.row_index + 1
    )
}

fn adjust_range_reference(
    start_col: &str,
    start_row: &str,
    end_col: &str,
    end_row: &str,
    shift: StructureShift,
) -> Option<(String, String)> {
    let mut start = parse_cell_reference(start_col, start_row)?;
    let mut end = parse_cell_reference(end_col, end_row)?;

    match shift {
        StructureShift::InsertRows { .. } | StructureShift::InsertColumns { .. } => {
            start = adjust_cell_reference_value(start, shift)?;
            end = adjust_cell_reference_value(end, shift)?;
        }
        StructureShift::DeleteRows { row_index, count } => {
            let (start_row, end_row) =
                adjust_deleted_range_axis(start.row_index, end.row_index, row_index, count)?;
            start.row_index = start_row;
            end.row_index = end_row;
        }
        StructureShift::DeleteColumns { col_index, count } => {
            let (start_col, end_col) =
                adjust_deleted_range_axis(start.col_index, end.col_index, col_index, count)?;
            start.col_index = start_col;
            end.col_index = end_col;
        }
    }

    Some((format_cell_reference(start), format_cell_reference(end)))
}

fn adjust_deleted_range_axis(
    start: usize,
    end: usize,
    delete_start: usize,
    count: usize,
) -> Option<(usize, usize)> {
    let delete_end = delete_start.checked_add(count)?.checked_sub(1)?;
    if start > end {
        return adjust_deleted_range_axis(end, start, delete_start, count)
            .map(|(end, start)| (start, end));
    }

    if end < delete_start {
        return Some((start, end));
    }
    if start > delete_end {
        return Some((start.saturating_sub(count), end.saturating_sub(count)));
    }

    let keeps_before = start < delete_start;
    let keeps_after = end > delete_end;
    match (keeps_before, keeps_after) {
        (false, false) => None,
        (true, false) => Some((start, delete_start.saturating_sub(1))),
        (false, true) => Some((delete_start, end.saturating_sub(count))),
        (true, true) => Some((start, end.saturating_sub(count))),
    }
}

fn adjust_cell_reference(col: &str, row: &str, shift: StructureShift) -> Option<String> {
    let cell_ref = parse_cell_reference(col, row)?;
    adjust_cell_reference_value(cell_ref, shift).map(format_cell_reference)
}

fn adjust_cell_reference_value(
    cell_ref: CellReference,
    shift: StructureShift,
) -> Option<CellReference> {
    let (new_col, new_row) = match shift {
        StructureShift::InsertRows {
            row_index: at,
            count,
        } => {
            let row = if cell_ref.row_index >= at {
                cell_ref.row_index + count
            } else {
                cell_ref.row_index
            };
            (cell_ref.col_index, row)
        }
        StructureShift::DeleteRows {
            row_index: at,
            count,
        } => {
            if (at..at + count).contains(&cell_ref.row_index) {
                return None;
            }
            let row = if cell_ref.row_index >= at + count {
                cell_ref.row_index - count
            } else {
                cell_ref.row_index
            };
            (cell_ref.col_index, row)
        }
        StructureShift::InsertColumns {
            col_index: at,
            count,
        } => {
            let col = if cell_ref.col_index >= at {
                cell_ref.col_index + count
            } else {
                cell_ref.col_index
            };
            (col, cell_ref.row_index)
        }
        StructureShift::DeleteColumns {
            col_index: at,
            count,
        } => {
            if (at..at + count).contains(&cell_ref.col_index) {
                return None;
            }
            let col = if cell_ref.col_index >= at + count {
                cell_ref.col_index - count
            } else {
                cell_ref.col_index
            };
            (col, cell_ref.row_index)
        }
    };

    Some(CellReference {
        col_index: new_col,
        row_index: new_row,
        ..cell_ref
    })
}

fn column_label_to_index(label: &str) -> Option<usize> {
    let mut value = 0usize;
    for byte in label.bytes() {
        if !byte.is_ascii_uppercase() {
            return None;
        }
        value = value
            .checked_mul(26)?
            .checked_add((byte - b'A' + 1) as usize)?;
    }
    value.checked_sub(1)
}

fn column_index_to_label(mut index: usize) -> String {
    let mut label = String::new();
    loop {
        let rem = index % 26;
        label.insert(0, (b'A' + rem as u8) as char);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    label
}

fn patch_cell(
    workbook: &mut Workbook,
    file_data: &FileData,
    sheet_index: usize,
    row: usize,
    col: usize,
) -> Result<(), AppError> {
    let Some(cell_value) = file_data
        .sheets
        .get(sheet_index)
        .and_then(|sheet| sheet.rows.get(row))
        .and_then(|row_data| row_data.get(col))
    else {
        return Ok(());
    };

    if let Some(worksheet) = sheet_mut(workbook, sheet_index)? {
        write_cell(worksheet, row as u32 + 1, col as u32 + 1, cell_value);
    }
    Ok(())
}

fn refresh_projection(workbook: &Workbook, file_data: &mut FileData) {
    file_data.sheets = workbook
        .sheet_collection()
        .iter()
        .map(read_worksheet)
        .collect();
}

fn patch_column_width(worksheet: &mut Worksheet, col_index: usize, width: Option<u32>) {
    let col_num = col_index as u32 + 1;
    match width {
        Some(width) => {
            worksheet
                .column_dimension_by_number_mut(col_num)
                .set_width(px_to_excel_column_width(width));
        }
        None => {
            worksheet
                .column_dimensions_mut()
                .retain(|column| column.col_num() != col_num);
        }
    }
}

fn patch_row_height(worksheet: &mut Worksheet, row_index: usize, height: Option<u32>) {
    let row_num = row_index as u32 + 1;
    match height {
        Some(height) => {
            worksheet
                .row_dimension_mut(row_num)
                .set_height(px_to_points(height));
        }
        None => {
            worksheet.row_dimensions_to_hashmap_mut().remove(&row_num);
        }
    }
}

fn insert_sheet(
    workbook: &mut Workbook,
    sheet_index: usize,
    sheet_data: &SheetData,
) -> Result<(), AppError> {
    let sheet_name = if sheet_data.name.is_empty() {
        format!("Sheet{}", sheet_index + 1)
    } else {
        sheet_data.name.clone()
    };

    workbook
        .new_sheet(sheet_name)
        .map_err(|e| AppError::WriteError(e.to_string()))?;

    let last_index = workbook.sheet_count().saturating_sub(1);
    if sheet_index < last_index {
        let sheets = workbook.sheet_collection_mut();
        for index in (sheet_index..last_index).rev() {
            sheets.swap(index, index + 1);
        }
    }

    let worksheet = workbook
        .sheet_mut(sheet_index)
        .map_err(|e| AppError::WriteError(e.to_string()))?;
    sync_sheet_from_sheet_data(worksheet, sheet_data)
}

fn remove_sheet(workbook: &mut Workbook, sheet_index: usize) -> Result<(), AppError> {
    if workbook.sheet_count() <= 1 {
        return Ok(());
    }
    if sheet_index < workbook.sheet_count() {
        workbook
            .remove_sheet(sheet_index)
            .map_err(|e| AppError::WriteError(e.to_string()))?;
    }
    Ok(())
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

fn sheet_name(workbook: &Workbook, sheet_index: usize) -> Result<String, AppError> {
    workbook
        .sheet(sheet_index)
        .map(|sheet| sheet.name().to_string())
        .map_err(|e| AppError::WriteError(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjusts_formula_references_for_inserted_columns() {
        assert_eq!(
            adjust_formula_references(
                "SUM(Inputs!A1:B2)",
                "Inputs",
                "Other",
                StructureShift::InsertColumns {
                    col_index: 1,
                    count: 1,
                },
            ),
            "SUM(Inputs!A1:C2)"
        );
    }

    #[test]
    fn adjusts_formula_references_for_deleted_rows() {
        assert_eq!(
            adjust_formula_references(
                "Inputs!A1+Inputs!A2",
                "Inputs",
                "Other",
                StructureShift::DeleteRows {
                    row_index: 1,
                    count: 1,
                },
            ),
            "Inputs!A1+Inputs!#REF!"
        );
    }

    #[test]
    fn leaves_reference_like_text_literals_unchanged() {
        assert_eq!(
            adjust_formula_references(
                r#""Inputs!A1"&Inputs!A1"#,
                "Inputs",
                "Other",
                StructureShift::InsertRows {
                    row_index: 0,
                    count: 1,
                },
            ),
            r#""Inputs!A1"&Inputs!A2"#
        );
    }

    #[test]
    fn shrinks_ranges_when_deleted_rows_touch_range_edges() {
        assert_eq!(
            adjust_formula_references(
                "SUM(Inputs!A1:A3)",
                "Inputs",
                "Other",
                StructureShift::DeleteRows {
                    row_index: 0,
                    count: 1,
                },
            ),
            "SUM(Inputs!A1:A2)"
        );

        assert_eq!(
            adjust_formula_references(
                "SUM(Inputs!A1:A3)",
                "Inputs",
                "Other",
                StructureShift::DeleteRows {
                    row_index: 2,
                    count: 1,
                },
            ),
            "SUM(Inputs!A1:A2)"
        );
    }

    #[test]
    fn shrinks_ranges_when_deleted_columns_touch_range_edges() {
        assert_eq!(
            adjust_formula_references(
                "SUM(Inputs!A1:C1)",
                "Inputs",
                "Other",
                StructureShift::DeleteColumns {
                    col_index: 0,
                    count: 1,
                },
            ),
            "SUM(Inputs!A1:B1)"
        );

        assert_eq!(
            adjust_formula_references(
                "SUM(Inputs!A1:C1)",
                "Inputs",
                "Other",
                StructureShift::DeleteColumns {
                    col_index: 2,
                    count: 1,
                },
            ),
            "SUM(Inputs!A1:B1)"
        );
    }

    #[test]
    fn removes_ranges_only_when_deleted_rows_cover_whole_range() {
        assert_eq!(
            adjust_formula_references(
                "SUM(Inputs!A1:A3)",
                "Inputs",
                "Other",
                StructureShift::DeleteRows {
                    row_index: 0,
                    count: 3,
                },
            ),
            "SUM(Inputs!#REF!)"
        );
    }

    #[test]
    fn adjusts_formula_references_with_locked_coordinates_and_quoted_sheets() {
        assert_eq!(
            adjust_formula_references(
                "'Input Sheet'!$A$1:$B2",
                "Input Sheet",
                "Other",
                StructureShift::InsertRows {
                    row_index: 1,
                    count: 1,
                },
            ),
            "'Input Sheet'!$A$1:$B3"
        );
    }

    #[test]
    fn leaves_other_sheet_references_unchanged() {
        assert_eq!(
            adjust_formula_references(
                "Other!A1+Inputs!A1",
                "Inputs",
                "Current",
                StructureShift::InsertRows {
                    row_index: 0,
                    count: 1,
                },
            ),
            "Other!A1+Inputs!A2"
        );
    }
}
