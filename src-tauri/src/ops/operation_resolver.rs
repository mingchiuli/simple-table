use crate::error::AppError;
use crate::ops::core_ops::{AppliedOperation, EditorCommand, ResolvedCellEdit};
use crate::types::{CellValue, FileData, SheetData, parse_cell_text};

impl EditorCommand {
    pub fn resolve(self, file_data: &FileData) -> Result<AppliedOperation, AppError> {
        match self {
            EditorCommand::SetCell {
                sheet_index,
                row,
                col,
                text,
            } => {
                require_sheet(file_data, sheet_index)?;
                let old_value = file_data.sheets[sheet_index]
                    .rows
                    .get(row)
                    .and_then(|row_data| row_data.get(col))
                    .cloned()
                    .unwrap_or(CellValue::Null);
                Ok(AppliedOperation::SetCell {
                    sheet_index,
                    row,
                    col,
                    old_value,
                    new_value: parse_cell_text(&text),
                })
            }
            EditorCommand::SetCells { changes } => {
                if changes.is_empty() {
                    return Ok(AppliedOperation::SetCells {
                        changes: Vec::new(),
                    });
                }
                let mut resolved = Vec::with_capacity(changes.len());
                for change in changes {
                    require_sheet(file_data, change.sheet_index)?;
                    let old_value = file_data.sheets[change.sheet_index]
                        .rows
                        .get(change.row)
                        .and_then(|row_data| row_data.get(change.col))
                        .cloned()
                        .unwrap_or(CellValue::Null);
                    resolved.push(ResolvedCellEdit {
                        sheet_index: change.sheet_index,
                        row: change.row,
                        col: change.col,
                        old_value,
                        new_value: parse_cell_text(&change.text),
                    });
                }
                Ok(AppliedOperation::SetCells { changes: resolved })
            }
            EditorCommand::AddRow {
                sheet_index,
                row_index,
            } => {
                let sheet = require_sheet(file_data, sheet_index)?;
                if row_index > sheet.rows.len() {
                    return Err(AppError::RowNotFound(row_index));
                }
                let col_count = sheet.rows.iter().map(Vec::len).max().unwrap_or(0);
                Ok(AppliedOperation::AddRow {
                    sheet_index,
                    row_index,
                    row_data: vec![CellValue::Null; col_count],
                    row_height: None,
                })
            }
            EditorCommand::DeleteRow {
                sheet_index,
                row_index,
            } => {
                let sheet = require_sheet(file_data, sheet_index)?;
                if row_index >= sheet_row_extent(sheet) {
                    return Err(AppError::RowNotFound(row_index));
                }
                Ok(AppliedOperation::DeleteRow {
                    sheet_index,
                    row_index,
                })
            }
            EditorCommand::AddColumn {
                sheet_index,
                col_index,
            } => {
                let sheet = require_sheet(file_data, sheet_index)?;
                if col_index > sheet_column_extent(sheet) {
                    return Err(AppError::InvalidCellPosition {
                        row: 0,
                        col: col_index,
                    });
                }
                Ok(AppliedOperation::AddColumn {
                    sheet_index,
                    col_index,
                    col_data: vec![CellValue::Null; sheet.rows.len()],
                    column_width: None,
                })
            }
            EditorCommand::DeleteColumn {
                sheet_index,
                col_index,
            } => {
                let sheet = require_sheet(file_data, sheet_index)?;
                let total_cols = sheet_column_extent(sheet);
                if col_index >= total_cols {
                    return Err(AppError::InvalidCellPosition {
                        row: 0,
                        col: col_index,
                    });
                }
                Ok(AppliedOperation::DeleteColumn {
                    sheet_index,
                    col_index,
                })
            }
            EditorCommand::SetColumnWidth {
                sheet_index,
                col_index,
                width,
            } => {
                let sheet = require_sheet(file_data, sheet_index)?;
                let old_width = sheet
                    .column_widths
                    .as_ref()
                    .and_then(|widths| widths.get(&col_index).copied());
                Ok(AppliedOperation::SetColumnWidth {
                    sheet_index,
                    col_index,
                    old_width,
                    new_width: width,
                })
            }
            EditorCommand::SetRowHeight {
                sheet_index,
                row_index,
                height,
            } => {
                let sheet = require_sheet(file_data, sheet_index)?;
                let old_height = sheet
                    .row_heights
                    .as_ref()
                    .and_then(|heights| heights.get(&row_index).copied());
                Ok(AppliedOperation::SetRowHeight {
                    sheet_index,
                    row_index,
                    old_height,
                    new_height: height,
                })
            }
            EditorCommand::AddSheet { name } => {
                let sheet_index = file_data.sheets.len();
                let sheet_name = name
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| format!("Sheet{}", sheet_index + 1));
                Ok(AppliedOperation::AddSheet {
                    sheet_index,
                    sheet_data: empty_sheet(sheet_name),
                })
            }
            EditorCommand::DeleteSheet { sheet_index } => {
                if file_data.sheets.len() <= 1 {
                    return Err(AppError::CannotDeleteLastSheet);
                }
                require_sheet(file_data, sheet_index)?;
                Ok(AppliedOperation::DeleteSheet { sheet_index })
            }
        }
    }
}

fn require_sheet(file_data: &FileData, sheet_index: usize) -> Result<&SheetData, AppError> {
    file_data
        .sheets
        .get(sheet_index)
        .ok_or(AppError::InvalidSheetIndex(sheet_index))
}

fn sheet_row_extent(sheet: &SheetData) -> usize {
    let row_count = sheet.rows.len();
    let merge_extent = sheet
        .merges
        .iter()
        .map(|merge| merge.end_row as usize + 1)
        .max()
        .unwrap_or(0);
    let layout_extent = sheet
        .row_heights
        .as_ref()
        .and_then(|heights| heights.keys().max().map(|index| index + 1))
        .unwrap_or(0);
    row_count.max(merge_extent).max(layout_extent)
}

fn sheet_column_extent(sheet: &SheetData) -> usize {
    let row_extent = sheet.rows.iter().map(Vec::len).max().unwrap_or(0);
    let merge_extent = sheet
        .merges
        .iter()
        .map(|merge| merge.end_col as usize + 1)
        .max()
        .unwrap_or(0);
    let layout_extent = sheet
        .column_widths
        .as_ref()
        .and_then(|widths| widths.keys().max().map(|index| index + 1))
        .unwrap_or(0);
    row_extent.max(merge_extent).max(layout_extent)
}

fn empty_sheet(name: String) -> SheetData {
    SheetData {
        name,
        rows: vec![vec![CellValue::Null; 5]; 5],
        merges: vec![],
        ..Default::default()
    }
}
