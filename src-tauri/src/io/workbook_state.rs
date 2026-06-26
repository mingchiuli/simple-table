use crate::error::AppError;
use crate::io::codec::writer::{
    sync_sheet_from_sheet_data, sync_workbook_from_file_data, write_cell,
};
use crate::ops::Operation;
use crate::types::{FileData, OperationResult, SheetCellChange, SheetData};
use umya_spreadsheet::{Workbook, Worksheet};

pub fn patch_after_operation(
    workbook: &mut Workbook,
    file_data: &FileData,
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
            ..
        } => {
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                worksheet.insert_new_row(*row_index as u32 + 1, 1);
                patch_sheet(worksheet, file_data, *sheet_index)?;
            }
            patch_cell_changes(workbook, file_data, cell_changes)?;
        }
        Operation::DeleteRow {
            sheet_index,
            row_index,
            ..
        } => {
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                worksheet.remove_row(*row_index as u32 + 1, 1);
                patch_sheet(worksheet, file_data, *sheet_index)?;
            }
            patch_cell_changes(workbook, file_data, cell_changes)?;
        }
        Operation::AddColumn {
            sheet_index,
            col_index,
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
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                worksheet.insert_new_column_by_index(actual_col_index as u32 + 1, 1);
                patch_sheet(worksheet, file_data, *sheet_index)?;
            }
            patch_cell_changes(workbook, file_data, cell_changes)?;
        }
        Operation::DeleteColumn {
            sheet_index,
            col_index,
            ..
        } => {
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                worksheet.remove_column_by_index(*col_index as u32 + 1, 1);
                patch_sheet(worksheet, file_data, *sheet_index)?;
            }
            patch_cell_changes(workbook, file_data, cell_changes)?;
        }
        Operation::SetColumnWidth { sheet_index, .. } => {
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                patch_sheet(worksheet, file_data, *sheet_index)?;
            }
        }
        Operation::SetRowHeight { sheet_index, .. } => {
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                patch_sheet(worksheet, file_data, *sheet_index)?;
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
                patch_cell_changes(workbook, file_data, cell_changes)?;
            }
        }
        Operation::DeleteSheet { .. } => {
            if let OperationResult::DeleteSheet { sheet_index, .. } = result {
                remove_sheet(workbook, *sheet_index)?;
                patch_cell_changes(workbook, file_data, cell_changes)?;
            }
        }
        Operation::SortColumn { sheet_index, .. } => {
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                patch_sheet(worksheet, file_data, *sheet_index)?;
            }
            patch_cell_changes(workbook, file_data, cell_changes)?;
        }
    }

    Ok(())
}

pub fn rebuild_from_file_data(
    workbook: &mut Workbook,
    file_data: &FileData,
) -> Result<(), AppError> {
    sync_workbook_from_file_data(workbook, file_data)
}

fn patch_cell_changes(
    workbook: &mut Workbook,
    file_data: &FileData,
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

fn patch_sheet(
    worksheet: &mut Worksheet,
    file_data: &FileData,
    sheet_index: usize,
) -> Result<(), AppError> {
    if let Some(sheet) = file_data.sheets.get(sheet_index) {
        sync_sheet_from_sheet_data(worksheet, sheet)?;
    }
    Ok(())
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
