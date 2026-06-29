use std::collections::HashMap;

use crate::error::AppError;
use crate::formula::reference_rewrite::{StructureShift, adjust_formula_references};
use crate::io::codec::reader::read_worksheet;
use crate::io::codec::writer::{
    coordinate, px_to_excel_column_width, px_to_points, sync_sheet_from_sheet_data, write_cell,
};
use crate::ops::AppliedOperation;
use crate::types::{AppliedOperationResult, FileData, SheetCellChange, SheetData};
use umya_spreadsheet::{Workbook, Worksheet};

pub fn patch_after_operation(
    workbook: &mut Workbook,
    file_data: &mut FileData,
    operation: &AppliedOperation,
    result: &AppliedOperationResult,
    cell_changes: &[SheetCellChange],
) -> Result<(), AppError> {
    match operation {
        AppliedOperation::SetCell {
            sheet_index,
            row,
            col,
            ..
        } => {
            patch_cell(workbook, file_data, *sheet_index, *row, *col)?;
            patch_cell_changes(workbook, file_data, cell_changes)?;
        }
        AppliedOperation::AddRow {
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
                sync_merge_ranges(worksheet, file_data, *sheet_index);
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
        AppliedOperation::DeleteRow {
            sheet_index,
            row_index,
            ..
        } => {
            let sheet_name = sheet_name(workbook, *sheet_index)?;
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                worksheet.remove_row(*row_index as u32 + 1, 1);
                sync_merge_ranges(worksheet, file_data, *sheet_index);
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
        AppliedOperation::AddColumn {
            sheet_index,
            col_index,
            col_data,
            column_width,
            ..
        } => {
            let actual_col_index = match result {
                AppliedOperationResult::AddColumn { column, .. } => column.index,
                _ => *col_index,
            };
            let sheet_name = sheet_name(workbook, *sheet_index)?;
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                worksheet.insert_new_column_by_index(actual_col_index as u32 + 1, 1);
                patch_column_cells(worksheet, actual_col_index, col_data);
                if let Some(width) = column_width {
                    patch_column_width(worksheet, actual_col_index, Some(*width));
                }
                sync_merge_ranges(worksheet, file_data, *sheet_index);
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
        AppliedOperation::DeleteColumn {
            sheet_index,
            col_index,
            ..
        } => {
            let sheet_name = sheet_name(workbook, *sheet_index)?;
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                worksheet.remove_column_by_index(*col_index as u32 + 1, 1);
                sync_merge_ranges(worksheet, file_data, *sheet_index);
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
        AppliedOperation::SetColumnWidth {
            sheet_index,
            col_index,
            new_width,
            ..
        } => {
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                patch_column_width(worksheet, *col_index, *new_width);
            }
        }
        AppliedOperation::SetRowHeight {
            sheet_index,
            row_index,
            new_height,
            ..
        } => {
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                patch_row_height(worksheet, *row_index, *new_height);
            }
        }
        AppliedOperation::AddSheet { .. } => {
            if let AppliedOperationResult::AddSheet {
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
        AppliedOperation::DeleteSheet { .. } => {
            if let AppliedOperationResult::DeleteSheet { sheet_index, .. } = result {
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

pub fn patch_layout_dimensions(
    workbook: &mut Workbook,
    sheet_index: usize,
    column_widths: &HashMap<usize, Option<u32>>,
    row_heights: &HashMap<usize, Option<u32>>,
) -> Result<(), AppError> {
    if let Some(worksheet) = sheet_mut(workbook, sheet_index)? {
        for (col_index, width) in column_widths {
            patch_column_width(worksheet, *col_index, *width);
        }
        for (row_index, height) in row_heights {
            patch_row_height(worksheet, *row_index, *height);
        }
    }
    Ok(())
}

pub fn patch_cell_shapes(
    workbook: &mut Workbook,
    sheet_shapes: &[(usize, Vec<usize>)],
) -> Result<(), AppError> {
    for (sheet_index, row_lengths) in sheet_shapes {
        let Some(worksheet) = sheet_mut(workbook, *sheet_index)? else {
            continue;
        };
        let (highest_col, highest_row) = worksheet.highest_column_and_row();
        let target_rows = row_lengths.len() as u32;

        for row in 1..=highest_row {
            let target_width = row
                .checked_sub(1)
                .and_then(|row_index| row_lengths.get(row_index as usize).copied())
                .unwrap_or(0) as u32;
            for col in 1..=highest_col {
                if row > target_rows || col > target_width {
                    worksheet.remove_cell((col, row));
                }
            }
        }
    }
    Ok(())
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

fn sync_merge_ranges(worksheet: &mut Worksheet, file_data: &FileData, sheet_index: usize) {
    worksheet.merge_cells_mut().clear();
    let Some(sheet) = file_data.sheets.get(sheet_index) else {
        return;
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

pub fn refresh_projection_from_workbook(workbook: &Workbook, file_data: &mut FileData) {
    file_data.sheets = workbook
        .sheet_collection()
        .iter()
        .map(read_worksheet)
        .collect();
}

fn refresh_projection(workbook: &Workbook, file_data: &mut FileData) {
    refresh_projection_from_workbook(workbook, file_data);
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
