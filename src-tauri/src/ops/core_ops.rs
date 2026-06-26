use std::cmp::Ordering;

use crate::types::{
    CellChange, CellValue, ColumnChange, ColumnWidthChange, FileData, OperationResult, RowChange,
    RowHeightChange, SheetData, SheetIndex, SortState,
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
    },
    /// 删除行
    DeleteRow {
        sheet_index: usize,
        row_index: usize,
        row_data: Vec<CellValue>,
    },
    /// 添加列
    AddColumn {
        sheet_index: usize,
        /// 记录添加的列索引，用于撤销
        col_index: Option<usize>,
        /// 添加的列数据（用于撤销时恢复）
        col_data: Vec<CellValue>,
    },
    /// 删除列
    DeleteColumn {
        sheet_index: usize,
        col_index: usize,
        col_data: Vec<CellValue>,
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
    /// 列排序（保存完整的 sheet 数据用于 undo）
    SortColumn {
        sheet_index: usize,
        col_index: usize,
        ascending: bool,
        /// 排序前的完整 sheet 数据（用于 undo 恢复）
        old_sheet_data: SheetData,
        /// 排序前的 sort_state（用于 undo 时恢复箭头状态）
        previous_sort_state: Option<SortState>,
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
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index)
                    && let Some(row_data) = sheet.rows.get_mut(*row)
                    && *col < row_data.len()
                {
                    // 先更新值
                    row_data[*col] = new_value.clone();
                    // 索引重建由调用方异步处理
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
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    // 使用传入的 row_data，如果为空则创建空行
                    let new_row = if row_data.is_empty() {
                        let col_count = sheet.rows.first().map(|r| r.len()).unwrap_or(0);
                        vec![CellValue::Null; col_count]
                    } else {
                        row_data.clone()
                    };
                    sheet.rows.insert(*row_index, new_row);
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
            } => {
                // 计算最终插入位置：优先使用传入的 col_index（来自 undo of DeleteColumn），
                // 否则按当前列数追加到末尾
                let actual_col_index = col_index.unwrap_or_else(|| {
                    file_data
                        .sheets
                        .get(*sheet_index)
                        .and_then(|s| s.rows.first())
                        .map(|r| r.len())
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
                        index: SheetIndex::default(),
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
            Operation::SortColumn {
                sheet_index,
                col_index,
                ascending,
                old_sheet_data,
                previous_sort_state,
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    // 比较 old_sheet_data 与当前 sheet 是否相同
                    // 如果相同：说明是正常排序操作（redo 时会走到这里）
                    // 如果不同：说明是 undo 恢复操作，需要用 old_sheet_data 替换
                    let is_restore = sheet.rows != old_sheet_data.rows;

                    if is_restore {
                        // undo 恢复：用 old_sheet_data 替换当前 sheet
                        *sheet = old_sheet_data.clone();
                        // 返回之前的 sort_state（用于恢复箭头显示）
                        OperationResult::SortColumn {
                            sheet_index: *sheet_index,
                            sheet_data: sheet.clone(),
                            sort_state: previous_sort_state.clone(),
                        }
                    } else {
                        // 正常排序：执行排序
                        sort_sheet(sheet, *col_index, *ascending);

                        let sort_state = SortState {
                            col_index: *col_index,
                            ascending: *ascending,
                        };

                        // 返回排序后的完整数据
                        OperationResult::SortColumn {
                            sheet_index: *sheet_index,
                            sheet_data: sheet.clone(),
                            sort_state: Some(sort_state),
                        }
                    }
                } else {
                    OperationResult::SortColumn {
                        sheet_index: *sheet_index,
                        sheet_data: old_sheet_data.clone(),
                        sort_state: previous_sort_state.clone(),
                    }
                }
            }
        }
    }
}

/// 对 sheet 按指定列排序
fn sort_sheet(sheet: &mut SheetData, col_index: usize, ascending: bool) {
    if sheet.rows.is_empty() || col_index >= sheet.rows.first().map(|r| r.len()).unwrap_or(0) {
        return;
    }

    // 获取列值用于排序
    let col_values: Vec<(usize, &CellValue)> = sheet
        .rows
        .iter()
        .enumerate()
        .map(|(i, row)| (i, row.get(col_index).unwrap_or(&CellValue::Null)))
        .collect();

    // 创建索引数组
    let mut indices: Vec<usize> = (0..sheet.rows.len()).collect();

    // 排序
    indices.sort_by(|&a, &b| {
        let val_a = col_values[a].1;
        let val_b = col_values[b].1;
        let cmp = compare_cell_values(val_a, val_b);
        if ascending { cmp } else { cmp.reverse() }
    });

    // 根据排序后的索引重新排列行
    let mut new_rows = Vec::with_capacity(sheet.rows.len());
    for idx in indices {
        new_rows.push(sheet.rows[idx].clone());
    }
    sheet.rows = new_rows;
}

/// 比较两个单元格值（用于排序）
/// 数字和字符串都支持按数值排序
fn compare_cell_values(a: &CellValue, b: &CellValue) -> Ordering {
    let mut a_normalized = None;
    let mut b_normalized = None;
    let a = sortable_cell_value(a, &mut a_normalized);
    let b = sortable_cell_value(b, &mut b_normalized);
    match (a, b) {
        // Null 排在最后
        (CellValue::Null, CellValue::Null) => Ordering::Equal,
        (CellValue::Null, _) => Ordering::Greater,
        (_, CellValue::Null) => Ordering::Less,

        // Number vs Number: 从 Value 中提取数字比较
        (CellValue::Number(na), CellValue::Number(nb)) => {
            // 优先尝试比较整数
            if let (Some(ia), Some(ib)) = (na.as_i64(), nb.as_i64()) {
                ia.cmp(&ib)
            } else if let (Some(fa), Some(fb)) = (na.as_f64(), nb.as_f64()) {
                fa.partial_cmp(&fb).unwrap_or(Ordering::Equal)
            } else {
                // 无法比较，按字符串比较
                na.to_string()
                    .to_lowercase()
                    .cmp(&nb.to_string().to_lowercase())
            }
        }
        // Number vs String: 尝试将 String 转为数字比较
        (CellValue::Number(na), CellValue::String(sb)) => {
            if let Ok(nb) = sb.parse::<f64>() {
                if let Some(fa) = na.as_f64() {
                    fa.partial_cmp(&nb).unwrap_or(Ordering::Equal)
                } else {
                    Ordering::Equal
                }
            } else {
                // 无法转为数字，按字符串比较
                na.to_string().to_lowercase().cmp(&sb.to_lowercase())
            }
        }
        (CellValue::String(sa), CellValue::Number(nb)) => {
            if let Ok(na) = sa.parse::<f64>() {
                if let Some(fb) = nb.as_f64() {
                    na.partial_cmp(&fb).unwrap_or(Ordering::Equal)
                } else {
                    Ordering::Equal
                }
            } else {
                sa.to_lowercase().cmp(&nb.to_string().to_lowercase())
            }
        }
        // String vs String: 尝试按数值排序，失败则按字典序
        (CellValue::String(sa), CellValue::String(sb)) => {
            // 优先尝试解析为整数
            if let (Ok(ia), Ok(ib)) = (sa.parse::<i128>(), sb.parse::<i128>()) {
                ia.cmp(&ib)
            } else if let (Ok(fa), Ok(fb)) = (sa.parse::<f64>(), sb.parse::<f64>()) {
                fa.partial_cmp(&fb).unwrap_or(Ordering::Equal)
            } else {
                // 忽略大小写排序
                sa.to_lowercase().cmp(&sb.to_lowercase())
            }
        }

        // 布尔值：true < false
        (CellValue::Boolean(ba), CellValue::Boolean(bb)) => ba.cmp(bb),
        (CellValue::Boolean(_), _) => Ordering::Greater,
        (_, CellValue::Boolean(_)) => Ordering::Less,
        (CellValue::Formula { .. }, _) | (_, CellValue::Formula { .. }) => a
            .to_display_string()
            .to_lowercase()
            .cmp(&b.to_display_string().to_lowercase()),
    }
}

fn sortable_cell_value<'a>(
    cell: &'a CellValue,
    normalized: &'a mut Option<CellValue>,
) -> &'a CellValue {
    match cell {
        CellValue::Formula {
            cached_value,
            error,
            ..
        } if error.is_none() => cached_value,
        CellValue::Formula { error, .. } => {
            *normalized = Some(CellValue::String(
                error.clone().unwrap_or_else(|| cell.to_display_string()),
            ));
            normalized.as_ref().unwrap()
        }
        _ => cell,
    }
}
