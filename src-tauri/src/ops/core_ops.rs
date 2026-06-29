use crate::types::{
    CellChange, CellValue, ColumnChange, ColumnWidthChange, FileData, MergeRange, OperationResult,
    RowChange, RowHeightChange, SheetData,
};
use serde::{Deserialize, Serialize};

/// 操作类型 - 用于执行和撤销/重做
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operation {
    /// 设置单元格值
    SetCell {
        sheet_index: usize,
        row: usize,
        col: usize,
        old_value: CellValue,
        new_value: CellValue,
    },
    /// 添加行
    AddRow {
        sheet_index: usize,
        row_index: usize,
        /// 被恢复的行数据（用于撤销 DeleteRow）
        row_data: Vec<CellValue>,
        row_height: Option<u32>,
    },
    /// 删除行
    DeleteRow {
        sheet_index: usize,
        row_index: usize,
        row_data: Vec<CellValue>,
        row_height: Option<u32>,
    },
    /// 添加列
    AddColumn {
        sheet_index: usize,
        /// 记录添加的列索引，用于撤销
        col_index: Option<usize>,
        /// 添加的列数据（用于撤销时恢复）
        col_data: Vec<CellValue>,
        column_width: Option<u32>,
    },
    /// 删除列
    DeleteColumn {
        sheet_index: usize,
        col_index: usize,
        col_data: Vec<CellValue>,
        column_width: Option<u32>,
    },
    SetColumnWidth {
        sheet_index: usize,
        col_index: usize,
        old_width: Option<u32>,
        new_width: Option<u32>,
    },
    SetRowHeight {
        sheet_index: usize,
        row_index: usize,
        old_height: Option<u32>,
        new_height: Option<u32>,
    },
    /// 添加 Sheet（带数据，用于撤销时恢复）
    AddSheet {
        /// sheet 名称（新建时使用）
        name: String,
        /// 完整的 sheet 数据（用于撤销恢复时）
        sheet_data: Option<SheetData>,
        /// 恢复时的原始索引（用于撤销 DeleteSheet 时恢复到正确位置）
        sheet_index: Option<usize>,
    },
    /// 删除 Sheet（带完整数据，用于撤销时恢复）
    DeleteSheet {
        sheet_index: usize,
        sheet_data: SheetData,
    },
}

impl Operation {
    /// 执行操作
    /// 注意：此方法不再同步重建索引，索引重建由调用方异步处理
    pub fn execute(&self, file_data: &mut FileData) -> OperationResult {
        match self {
            Operation::SetCell {
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
                OperationResult::SetCell {
                    sheet_index: *sheet_index,
                    cell: CellChange {
                        row: *row,
                        col: *col,
                        value: new_value.clone(),
                    },
                }
            }
            Operation::AddRow {
                sheet_index,
                row_index,
                row_data,
                row_height,
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    // 使用传入的 row_data，如果为空则创建空行
                    let new_row = if row_data.is_empty() {
                        let col_count = sheet.rows.iter().map(Vec::len).max().unwrap_or(0);
                        vec![CellValue::Null; col_count]
                    } else {
                        row_data.clone()
                    };
                    sheet.rows.insert(*row_index, new_row);
                    shift_layout_map_on_insert(sheet.row_heights.as_mut(), *row_index);
                    shift_row_merges_on_insert(&mut sheet.merges, *row_index);
                    if let Some(height) = row_height {
                        sheet
                            .row_heights
                            .get_or_insert_with(Default::default)
                            .insert(*row_index, *height);
                    }
                    // 索引重建由调用方异步处理
                }
                OperationResult::AddRow {
                    sheet_index: *sheet_index,
                    row: RowChange {
                        index: *row_index,
                        values: row_data.clone(),
                    },
                }
            }
            Operation::DeleteRow {
                sheet_index,
                row_index,
                ..
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index)
                    && *row_index < sheet.rows.len()
                {
                    sheet.rows.remove(*row_index);
                    shift_layout_map_on_delete(sheet.row_heights.as_mut(), *row_index);
                    shift_row_merges_on_delete(&mut sheet.merges, *row_index);
                    // 索引重建由调用方异步处理
                }
                OperationResult::DeleteRow {
                    sheet_index: *sheet_index,
                    row_index: *row_index,
                }
            }
            Operation::AddColumn {
                sheet_index,
                col_index,
                col_data,
                column_width,
            } => {
                // 计算最终插入位置：优先使用传入的 col_index（来自 undo of DeleteColumn），
                // 否则按当前列数追加到末尾
                let actual_col_index = col_index.unwrap_or_else(|| {
                    file_data
                        .sheets
                        .get(*sheet_index)
                        .map(|s| s.rows.iter().map(Vec::len).max().unwrap_or(0))
                        .unwrap_or(0)
                });
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    // 使用传入的 col_data，如果为空则创建空列
                    let new_col_data = if col_data.is_empty() {
                        vec![CellValue::Null; sheet.rows.len()]
                    } else {
                        col_data.clone()
                    };
                    // 按 actual_col_index 插入（>= 当前列数时退化为末尾追加）
                    for (i, row) in sheet.rows.iter_mut().enumerate() {
                        let value = new_col_data.get(i).cloned().unwrap_or(CellValue::Null);
                        let pos = actual_col_index.min(row.len());
                        row.insert(pos, value);
                    }
                    shift_layout_map_on_insert(sheet.column_widths.as_mut(), actual_col_index);
                    shift_column_merges_on_insert(&mut sheet.merges, actual_col_index);
                    if let Some(width) = column_width {
                        sheet
                            .column_widths
                            .get_or_insert_with(Default::default)
                            .insert(actual_col_index, *width);
                    }
                    // 索引重建由调用方异步处理
                }
                OperationResult::AddColumn {
                    sheet_index: *sheet_index,
                    column: ColumnChange {
                        index: actual_col_index,
                    },
                    col_data: col_data.clone(),
                }
            }
            Operation::DeleteColumn {
                sheet_index,
                col_index,
                ..
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    for row in &mut sheet.rows {
                        if *col_index < row.len() {
                            row.remove(*col_index);
                        }
                    }
                    shift_layout_map_on_delete(sheet.column_widths.as_mut(), *col_index);
                    shift_column_merges_on_delete(&mut sheet.merges, *col_index);
                    // 索引重建由调用方异步处理
                }
                OperationResult::DeleteColumn {
                    sheet_index: *sheet_index,
                    column_index: *col_index,
                }
            }
            Operation::SetColumnWidth {
                sheet_index,
                col_index,
                new_width,
                ..
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    match new_width {
                        Some(width) => {
                            sheet
                                .column_widths
                                .get_or_insert_with(Default::default)
                                .insert(*col_index, *width);
                        }
                        None => {
                            if let Some(widths) = sheet.column_widths.as_mut() {
                                widths.remove(col_index);
                                if widths.is_empty() {
                                    sheet.column_widths = None;
                                }
                            }
                        }
                    }
                }
                OperationResult::SetColumnWidth {
                    sheet_index: *sheet_index,
                    column: ColumnWidthChange {
                        col_index: *col_index,
                        width: *new_width,
                    },
                }
            }
            Operation::SetRowHeight {
                sheet_index,
                row_index,
                new_height,
                ..
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    match new_height {
                        Some(height) => {
                            sheet
                                .row_heights
                                .get_or_insert_with(Default::default)
                                .insert(*row_index, *height);
                        }
                        None => {
                            if let Some(heights) = sheet.row_heights.as_mut() {
                                heights.remove(row_index);
                                if heights.is_empty() {
                                    sheet.row_heights = None;
                                }
                            }
                        }
                    }
                }
                OperationResult::SetRowHeight {
                    sheet_index: *sheet_index,
                    row: RowHeightChange {
                        row_index: *row_index,
                        height: *new_height,
                    },
                }
            }
            Operation::AddSheet {
                name,
                sheet_data,
                sheet_index,
            } => {
                // 如果有完整的 sheet_data，直接插入；否则创建空 sheet
                let (new_sheet, sheet_name) = if let Some(data) = sheet_data {
                    (data.clone(), data.name.clone())
                } else {
                    // 生成新 sheet 名称
                    let final_name = if name.is_empty() {
                        let sheet_count = file_data.sheets.len();
                        format!("Sheet{}", sheet_count + 1)
                    } else {
                        name.clone()
                    };

                    // 创建新的空 sheet
                    let new_sheet = SheetData {
                        name: final_name.clone(),
                        rows: vec![
                            vec![CellValue::Null; 5],
                            vec![CellValue::Null; 5],
                            vec![CellValue::Null; 5],
                            vec![CellValue::Null; 5],
                            vec![CellValue::Null; 5],
                        ],
                        merges: vec![],
                        ..Default::default()
                    };
                    (new_sheet, final_name)
                };

                // 如果提供了 sheet_index，插入到指定位置；否则添加到末尾
                let actual_index = sheet_index.unwrap_or(file_data.sheets.len());
                file_data.sheets.insert(actual_index, new_sheet.clone());

                OperationResult::AddSheet {
                    sheet_index: actual_index,
                    name: sheet_name,
                    sheet_data: new_sheet,
                }
            }
            Operation::DeleteSheet {
                sheet_index,
                sheet_data: _,
            } => {
                // 如果 sheet_index 是 MAX，说明这是 AddSheet 的撤销操作，需要删除最后一个 sheet
                let actual_index = if *sheet_index == usize::MAX {
                    file_data.sheets.len().saturating_sub(1)
                } else {
                    *sheet_index
                };

                let removed_sheet = file_data.sheets.remove(actual_index);

                OperationResult::DeleteSheet {
                    sheet_index: actual_index,
                    sheet_data: removed_sheet,
                }
            }
        }
    }
}

fn shift_layout_map_on_insert(
    map: Option<&mut std::collections::HashMap<usize, u32>>,
    index: usize,
) {
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

fn shift_layout_map_on_delete(
    map: Option<&mut std::collections::HashMap<usize, u32>>,
    index: usize,
) {
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
