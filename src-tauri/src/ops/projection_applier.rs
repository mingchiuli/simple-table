use std::collections::HashMap;

use crate::ops::core_ops::{AppliedOperation, ProjectionMutation};
use crate::types::{
    AppliedOperationResult, CellChange, CellValue, ColumnChange, ColumnWidthChange, FileData,
    MergeRange, RowChange, RowHeightChange, SheetCellChange, SheetData,
};

impl ProjectionMutation<'_> {
    pub fn execute(&self, file_data: &mut FileData) -> AppliedOperationResult {
        match self.operation {
            AppliedOperation::SetCell {
                sheet_index,
                row,
                col,
                new_value,
                ..
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    ensure_cell_exists(sheet, *row, *col);
                    sheet.rows[*row][*col] = new_value.clone();
                }
                AppliedOperationResult::SetCell {
                    sheet_index: *sheet_index,
                    cell: CellChange {
                        row: *row,
                        col: *col,
                        value: new_value.clone(),
                    },
                }
            }
            AppliedOperation::SetCells { changes } => {
                for change in changes {
                    if let Some(sheet) = file_data.sheets.get_mut(change.sheet_index) {
                        ensure_cell_exists(sheet, change.row, change.col);
                        sheet.rows[change.row][change.col] = change.new_value.clone();
                    }
                }
                AppliedOperationResult::SetCells {
                    changes: changes
                        .iter()
                        .map(|change| {
                            SheetCellChange::new(
                                change.sheet_index,
                                change.row,
                                change.col,
                                change.new_value.clone(),
                            )
                        })
                        .collect(),
                }
            }
            AppliedOperation::AddRow {
                sheet_index,
                row_index,
                row_data,
                row_height,
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    while sheet.rows.len() < *row_index {
                        sheet.rows.push(Vec::new());
                    }
                    sheet.rows.insert(*row_index, row_data.clone());
                    shift_layout_map_on_insert(sheet.row_heights.as_mut(), *row_index);
                    shift_row_merges_on_insert(&mut sheet.merges, *row_index);
                    if let Some(height) = row_height {
                        sheet
                            .row_heights
                            .get_or_insert_with(Default::default)
                            .insert(*row_index, *height);
                    }
                }
                AppliedOperationResult::AddRow {
                    sheet_index: *sheet_index,
                    row: RowChange {
                        index: *row_index,
                        values: row_data.clone(),
                    },
                }
            }
            AppliedOperation::DeleteRow {
                sheet_index,
                row_index,
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    if *row_index < sheet.rows.len() {
                        sheet.rows.remove(*row_index);
                    }
                    shift_layout_map_on_delete(sheet.row_heights.as_mut(), *row_index);
                    shift_row_merges_on_delete(&mut sheet.merges, *row_index);
                }
                AppliedOperationResult::DeleteRow {
                    sheet_index: *sheet_index,
                    row_index: *row_index,
                }
            }
            AppliedOperation::AddColumn {
                sheet_index,
                col_index,
                col_data,
                column_width,
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    if sheet.rows.len() < col_data.len() {
                        sheet.rows.resize_with(col_data.len(), Vec::new);
                    }
                    for (row_index, row) in sheet.rows.iter_mut().enumerate() {
                        let value = col_data.get(row_index).cloned().unwrap_or(CellValue::Null);
                        let pos = (*col_index).min(row.len());
                        row.insert(pos, value);
                    }
                    shift_layout_map_on_insert(sheet.column_widths.as_mut(), *col_index);
                    shift_column_merges_on_insert(&mut sheet.merges, *col_index);
                    if let Some(width) = column_width {
                        sheet
                            .column_widths
                            .get_or_insert_with(Default::default)
                            .insert(*col_index, *width);
                    }
                }
                AppliedOperationResult::AddColumn {
                    sheet_index: *sheet_index,
                    column: ColumnChange { index: *col_index },
                    col_data: col_data.clone(),
                }
            }
            AppliedOperation::DeleteColumn {
                sheet_index,
                col_index,
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    for row in &mut sheet.rows {
                        if *col_index < row.len() {
                            row.remove(*col_index);
                        }
                    }
                    shift_layout_map_on_delete(sheet.column_widths.as_mut(), *col_index);
                    shift_column_merges_on_delete(&mut sheet.merges, *col_index);
                }
                AppliedOperationResult::DeleteColumn {
                    sheet_index: *sheet_index,
                    column_index: *col_index,
                }
            }
            AppliedOperation::SetColumnWidth {
                sheet_index,
                col_index,
                new_width,
                ..
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    set_layout_value(&mut sheet.column_widths, *col_index, *new_width);
                }
                AppliedOperationResult::SetColumnWidth {
                    sheet_index: *sheet_index,
                    column: ColumnWidthChange {
                        col_index: *col_index,
                        width: *new_width,
                    },
                }
            }
            AppliedOperation::SetRowHeight {
                sheet_index,
                row_index,
                new_height,
                ..
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    set_layout_value(&mut sheet.row_heights, *row_index, *new_height);
                }
                AppliedOperationResult::SetRowHeight {
                    sheet_index: *sheet_index,
                    row: RowHeightChange {
                        row_index: *row_index,
                        height: *new_height,
                    },
                }
            }
            AppliedOperation::AddSheet {
                sheet_index,
                sheet_data,
            } => {
                let index = (*sheet_index).min(file_data.sheets.len());
                file_data.sheets.insert(index, sheet_data.clone());
                AppliedOperationResult::AddSheet {
                    sheet_index: index,
                    name: sheet_data.name.clone(),
                    sheet_data: sheet_data.clone(),
                }
            }
            AppliedOperation::DeleteSheet { sheet_index } => {
                let removed_sheet = file_data.sheets.remove(*sheet_index);
                AppliedOperationResult::DeleteSheet {
                    sheet_index: *sheet_index,
                    sheet_data: removed_sheet,
                }
            }
        }
    }

    pub fn execute_cells_and_layout(
        &self,
        file_data: &mut FileData,
    ) -> Option<AppliedOperationResult> {
        match self.operation {
            AppliedOperation::SetCell { .. }
            | AppliedOperation::SetCells { .. }
            | AppliedOperation::SetColumnWidth { .. }
            | AppliedOperation::SetRowHeight { .. } => Some(self.execute(file_data)),
            AppliedOperation::AddRow { .. }
            | AppliedOperation::DeleteRow { .. }
            | AppliedOperation::AddColumn { .. }
            | AppliedOperation::DeleteColumn { .. }
            | AppliedOperation::AddSheet { .. }
            | AppliedOperation::DeleteSheet { .. } => None,
        }
    }
}

fn set_layout_value(map: &mut Option<HashMap<usize, u32>>, index: usize, value: Option<u32>) {
    match value {
        Some(value) => {
            map.get_or_insert_with(Default::default)
                .insert(index, value);
        }
        None => {
            if let Some(values) = map.as_mut() {
                values.remove(&index);
                if values.is_empty() {
                    *map = None;
                }
            }
        }
    }
}

fn shift_layout_map_on_insert(map: Option<&mut HashMap<usize, u32>>, index: usize) {
    let Some(map) = map else {
        return;
    };
    if map.is_empty() {
        return;
    }
    let shifted = map
        .drain()
        .map(|(key, value)| {
            let key = if key >= index { key + 1 } else { key };
            (key, value)
        })
        .collect();
    *map = shifted;
}

fn ensure_cell_exists(sheet: &mut SheetData, row: usize, col: usize) {
    let target_width = col + 1;
    while sheet.rows.len() <= row {
        sheet.rows.push(vec![CellValue::Null; target_width]);
    }
    for row_data in &mut sheet.rows {
        if row_data.len() < target_width {
            row_data.resize(target_width, CellValue::Null);
        }
    }
}

fn shift_row_merges_on_insert(merges: &mut Vec<MergeRange>, row_index: usize) {
    let row = row_index as u32;
    for merge in merges {
        if merge.start_row >= row {
            merge.start_row += 1;
            merge.end_row += 1;
        } else if merge.end_row >= row {
            merge.end_row += 1;
        }
    }
}

fn shift_row_merges_on_delete(merges: &mut Vec<MergeRange>, row_index: usize) {
    let row = row_index as u32;
    merges.retain_mut(|merge| {
        if merge.start_row == row && merge.end_row == row {
            return false;
        }
        if merge.start_row > row {
            merge.start_row -= 1;
            merge.end_row -= 1;
        } else if merge.end_row >= row {
            merge.end_row = merge.end_row.saturating_sub(1);
        }
        merge.start_row <= merge.end_row && merge.start_col <= merge.end_col
    });
}

fn shift_column_merges_on_insert(merges: &mut Vec<MergeRange>, col_index: usize) {
    let col = col_index as u16;
    for merge in merges {
        if merge.start_col >= col {
            merge.start_col += 1;
            merge.end_col += 1;
        } else if merge.end_col >= col {
            merge.end_col += 1;
        }
    }
}

fn shift_column_merges_on_delete(merges: &mut Vec<MergeRange>, col_index: usize) {
    let col = col_index as u16;
    merges.retain_mut(|merge| {
        if merge.start_col == col && merge.end_col == col {
            return false;
        }
        if merge.start_col > col {
            merge.start_col -= 1;
            merge.end_col -= 1;
        } else if merge.end_col >= col {
            merge.end_col = merge.end_col.saturating_sub(1);
        }
        merge.start_row <= merge.end_row && merge.start_col <= merge.end_col
    });
}

fn shift_layout_map_on_delete(map: Option<&mut HashMap<usize, u32>>, index: usize) {
    let Some(map) = map else {
        return;
    };
    if map.is_empty() {
        return;
    }
    let shifted = map
        .drain()
        .filter_map(|(key, value)| {
            if key == index {
                None
            } else {
                let key = if key > index { key - 1 } else { key };
                Some((key, value))
            }
        })
        .collect();
    *map = shifted;
}
