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

    re.replace_all(formula, |captures: &Captures<'_>| {
        let sheet_name = captures
            .name("quoted")
            .map(|m| m.as_str().replace("''", "'"))
            .or_else(|| captures.name("sheet").map(|m| m.as_str().to_string()));

        let applies_to_reference = sheet_name
            .as_deref()
            .map(|name| name == target_sheet_name)
            .unwrap_or(current_sheet_name == target_sheet_name);
        if !applies_to_reference {
            return captures[0].to_string();
        }

        let Some(start) =
            adjust_cell_reference(&captures["start_col"], &captures["start_row"], shift)
        else {
            return format!(
                "{}#REF!",
                captures.name("prefix").map(|m| m.as_str()).unwrap_or("")
            );
        };

        let Some(end_col) = captures.name("end_col") else {
            return format!(
                "{}{}",
                captures.name("prefix").map(|m| m.as_str()).unwrap_or(""),
                start
            );
        };
        let Some(end_row) = captures.name("end_row") else {
            return format!(
                "{}{}",
                captures.name("prefix").map(|m| m.as_str()).unwrap_or(""),
                start
            );
        };

        let Some(end) = adjust_cell_reference(end_col.as_str(), end_row.as_str(), shift) else {
            return format!(
                "{}#REF!",
                captures.name("prefix").map(|m| m.as_str()).unwrap_or("")
            );
        };

        format!(
            "{}{}:{}",
            captures.name("prefix").map(|m| m.as_str()).unwrap_or(""),
            start,
            end
        )
    })
    .into_owned()
}

fn adjust_cell_reference(col: &str, row: &str, shift: StructureShift) -> Option<String> {
    let col_locked = col.starts_with('$');
    let row_locked = row.starts_with('$');
    let col_index = column_label_to_index(col.trim_start_matches('$'))?;
    let row_index = row
        .trim_start_matches('$')
        .parse::<usize>()
        .ok()?
        .checked_sub(1)?;

    let (new_col, new_row) = match shift {
        StructureShift::InsertRows {
            row_index: at,
            count,
        } => {
            let row = if row_index >= at {
                row_index + count
            } else {
                row_index
            };
            (col_index, row)
        }
        StructureShift::DeleteRows {
            row_index: at,
            count,
        } => {
            if (at..at + count).contains(&row_index) {
                return None;
            }
            let row = if row_index >= at + count {
                row_index - count
            } else {
                row_index
            };
            (col_index, row)
        }
        StructureShift::InsertColumns {
            col_index: at,
            count,
        } => {
            let col = if col_index >= at {
                col_index + count
            } else {
                col_index
            };
            (col, row_index)
        }
        StructureShift::DeleteColumns {
            col_index: at,
            count,
        } => {
            if (at..at + count).contains(&col_index) {
                return None;
            }
            let col = if col_index >= at + count {
                col_index - count
            } else {
                col_index
            };
            (col, row_index)
        }
    };

    Some(format!(
        "{}{}{}{}",
        if col_locked { "$" } else { "" },
        column_index_to_label(new_col),
        if row_locked { "$" } else { "" },
        new_row + 1
    ))
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
