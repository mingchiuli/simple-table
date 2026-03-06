use serde::{Deserialize, Serialize};
use crate::types::{CellPosition, CellValue, ColumnChange, OperationResult, RowChange, SheetData, SortState};

/// 将单元格值转换为字符串
fn cell_to_string(cell: &CellValue) -> String {
    match cell {
        CellValue::Null => String::new(),
        CellValue::String(s) => s.clone(),
        CellValue::Number(n) => n.to_string(),
        CellValue::Boolean(b) => b.to_string(),
    }
}

/// 更新单个单元格的索引
fn update_cell_index(sheet: &mut SheetData, row: usize, col: usize, old_value: &CellValue, new_value: &CellValue) {
    let old_text = cell_to_string(old_value);
    let new_text = cell_to_string(new_value);

    // 如果值没变，不需要更新
    if old_text.to_lowercase() == new_text.to_lowercase() {
        return;
    }

    // 删除旧值的索引
    if !old_text.is_empty() {
        let old_token = old_text.to_lowercase();
        if let Some(positions) = sheet.index.inverted_index.get_mut(&old_token) {
            positions.retain(|p| !(p.row == row && p.col == col));
            if positions.is_empty() {
                sheet.index.inverted_index.remove(&old_token);
            }
        }
    }

    // 添加新值的索引
    if !new_text.is_empty() {
        let new_token = new_text.to_lowercase();
        sheet.index.inverted_index
            .entry(new_token)
            .or_default()
            .push(CellPosition { row, col });
    }
}

/// 对 sheet 按指定列排序
fn sort_sheet(sheet: &mut SheetData, col_index: usize, ascending: bool) {
    if sheet.rows.is_empty() || col_index >= sheet.rows.first().map(|r| r.len()).unwrap_or(0) {
        return;
    }

    // 获取列值用于排序
    let col_values: Vec<(usize, &CellValue)> = sheet.rows.iter()
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
fn compare_cell_values(a: &CellValue, b: &CellValue) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        // Null 排在最后
        (CellValue::Null, CellValue::Null) => Ordering::Equal,
        (CellValue::Null, _) => Ordering::Greater,
        (_, CellValue::Null) => Ordering::Less,
        // 数字
        (CellValue::Number(na), CellValue::Number(nb)) => {
            na.partial_cmp(nb).unwrap_or(Ordering::Equal)
        }
        (CellValue::Number(_), _) => Ordering::Greater,
        (_, CellValue::Number(_)) => Ordering::Less,
        // 布尔值：true < false
        (CellValue::Boolean(ba), CellValue::Boolean(bb)) => {
            ba.cmp(bb)
        }
        (CellValue::Boolean(_), _) => Ordering::Greater,
        (_, CellValue::Boolean(_)) => Ordering::Less,
        // 字符串：按字典序
        (CellValue::String(sa), CellValue::String(sb)) => {
            // 忽略大小写排序
            sa.to_lowercase().cmp(&sb.to_lowercase())
        }
    }
}

/// 操作类型 - 用于撤销/重做
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

/// Trait for operations that can be undone/redone
/// Each operation can define its own behavior for creating redo operations
pub trait Undoable {
    /// Execute the undo operation on file_data
    fn undo(&self, file_data: &mut crate::types::FileData) -> OperationResult;

    /// Get the operation to be pushed to redo_stack after undo
    /// This allows operations to customize their redo behavior
    fn get_redo_operation(&self, file_data: &mut crate::types::FileData) -> Operation;
}

impl Undoable for Operation {
    /// Execute undo operation
    fn undo(&self, file_data: &mut crate::types::FileData) -> OperationResult {
        let undo_op = self.create_undo_op();
        undo_op.execute(file_data)
    }

    /// Get the redo operation after undo
    /// Default implementation returns self.clone()
    /// SortColumn overrides this to update old_sheet_data to current state
    fn get_redo_operation(&self, file_data: &mut crate::types::FileData) -> Operation {
        match self {
            // SortColumn: update old_sheet_data to current (sorted) state for redo
            Operation::SortColumn { sheet_index, col_index, ascending, old_sheet_data: _, previous_sort_state } => {
                if let Some(sheet) = file_data.sheets.get(*sheet_index) {
                    Operation::SortColumn {
                        sheet_index: *sheet_index,
                        col_index: *col_index,
                        ascending: *ascending,
                        old_sheet_data: sheet.clone(), // Current state becomes old for redo
                        previous_sort_state: previous_sort_state.clone(),
                    }
                } else {
                    self.clone()
                }
            }
            // Default: return self unchanged
            _ => self.clone()
        }
    }
}

impl Operation {
    /// 执行操作
    /// 注意：此方法不再同步重建索引，索引重建由调用方异步处理
    pub fn execute(&self, file_data: &mut crate::types::FileData) -> OperationResult {
        use crate::types::CellChange;

        match self {
            Operation::SetCell { sheet_index, row, col, new_value, .. } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    // 先获取旧值
                    let old_val = sheet.rows.get(*row)
                        .and_then(|r| r.get(*col))
                        .cloned()
                        .unwrap_or(CellValue::Null);

                    if let Some(row_data) = sheet.rows.get_mut(*row) {
                        if *col < row_data.len() {
                            // 先更新值
                            row_data[*col] = new_value.clone();
                            // 增量更新索引（同步执行，因为是单单元格操作，开销小）
                            update_cell_index(sheet, *row, *col, &old_val, new_value);
                        }
                    }
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
            Operation::AddRow { sheet_index, row_index, row_data } => {
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
            Operation::DeleteRow { sheet_index, row_index, .. } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    if *row_index < sheet.rows.len() {
                        sheet.rows.remove(*row_index);
                    }
                    // 索引重建由调用方异步处理
                }
                OperationResult::DeleteRow {
                    sheet_index: *sheet_index,
                    row_index: *row_index,
                }
            }
            Operation::AddColumn { sheet_index, col_index, col_data } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    // 使用传入的 col_data，如果为空则创建空列
                    let new_col_data = if col_data.is_empty() {
                        vec![CellValue::Null; sheet.rows.len()]
                    } else {
                        col_data.clone()
                    };
                    // 添加列数据到每一行
                    for (i, row) in sheet.rows.iter_mut().enumerate() {
                        if i < new_col_data.len() {
                            row.push(new_col_data[i].clone());
                        } else {
                            row.push(CellValue::Null);
                        }
                    }
                    // 索引重建由调用方异步处理
                }
                // 使用传入的 col_index 或计算最后一列的索引
                let actual_col_index = col_index.unwrap_or_else(|| {
                    file_data.sheets
                        .get(*sheet_index)
                        .and_then(|s| s.rows.first())
                        .map(|r| r.len().saturating_sub(1))
                        .unwrap_or(0)
                });
                OperationResult::AddColumn {
                    sheet_index: *sheet_index,
                    column: ColumnChange { index: actual_col_index },
                    col_data: col_data.clone(),
                }
            }
            Operation::DeleteColumn { sheet_index, col_index, .. } => {
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
            Operation::AddSheet { name, sheet_data, sheet_index } => {
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
                        index: crate::types::SheetIndex::default(),
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
            Operation::DeleteSheet { sheet_index, sheet_data } => {
                // Don't allow deleting the last sheet (仅对正常删除操作)
                if file_data.sheets.len() <= 1 && sheet_data.is_empty() {
                    return OperationResult::AddSheet {
                        sheet_index: 0,
                        name: "Error".to_string(),
                        sheet_data: SheetData::default(),
                    };
                }

                // 如果 sheet_index 是 MAX，说明这是 AddSheet 的撤销操作，需要删除最后一个 sheet
                let actual_index = if *sheet_index == usize::MAX {
                    file_data.sheets.len().saturating_sub(1)
                } else {
                    *sheet_index
                };

                let removed_sheet = file_data.sheets.remove(actual_index);

                // Adjust current sheet index if needed
                let new_current_index = if actual_index >= file_data.sheets.len() {
                    file_data.sheets.len().saturating_sub(1)
                } else {
                    actual_index
                };

                OperationResult::DeleteSheet {
                    sheet_index: new_current_index,
                    sheet_data: removed_sheet,
                }
            }
            Operation::SortColumn { sheet_index, col_index, ascending, old_sheet_data, previous_sort_state } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    // 比较 old_sheet_data 与当前 sheet 是否相同
                    // 如果相同：说明是正常排序操作（redo 时会走到这里）
                    // 如果不同：说明是 undo 恢复操作，需要用 old_sheet_data 替换
                    let is_restore = sheet.rows != old_sheet_data.rows;

                    if is_restore {
                        // undo 恢复：用 old_sheet_data 替换当前 sheet
                        *sheet = old_sheet_data.clone();
                        // 返回之前的 sort_state（用于恢复箭头显示）
                        eprintln!("[execute] is_restore=true, previous_sort_state: {:?}", previous_sort_state);
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

    /// 创建撤销操作（返回反向操作）
    pub fn create_undo_op(&self) -> Operation {
        match self {
            Operation::SetCell { sheet_index, row, col, old_value, new_value } => {
                Operation::SetCell {
                    sheet_index: *sheet_index,
                    row: *row,
                    col: *col,
                    old_value: new_value.clone(),
                    new_value: old_value.clone(),
                }
            }
            Operation::AddRow { sheet_index, row_index, row_data } => {
                Operation::DeleteRow {
                    sheet_index: *sheet_index,
                    row_index: *row_index,
                    row_data: row_data.clone(), // 保留添加的行数据，用于撤销 DeleteRow 时恢复
                }
            }
            Operation::DeleteRow { sheet_index, row_index, row_data } => {
                Operation::AddRow {
                    sheet_index: *sheet_index,
                    row_index: *row_index,
                    row_data: row_data.clone(),
                }
            }
            Operation::AddColumn { sheet_index, col_index, col_data } => {
                Operation::DeleteColumn {
                    sheet_index: *sheet_index,
                    // 使用添加列时记录的索引
                    col_index: col_index.unwrap_or(0),
                    col_data: col_data.clone(),
                }
            }
            Operation::DeleteColumn { sheet_index, col_index, col_data } => {
                Operation::AddColumn {
                    sheet_index: *sheet_index,
                    col_index: Some(*col_index),
                    col_data: col_data.clone(),
                }
            }
            Operation::AddSheet { .. } => {
                // AddSheet 的撤销：删除最后添加的 sheet（新建的 sheet 是空的，不需要保存数据）
                Operation::DeleteSheet {
                    sheet_index: usize::MAX,
                    sheet_data: SheetData::default(),
                }
            }
            Operation::DeleteSheet { sheet_index, sheet_data } => {
                // DeleteSheet 的撤销：恢复被删除的 sheet（使用保存的完整数据）
                Operation::AddSheet {
                    name: sheet_data.name.clone(),
                    sheet_data: Some(sheet_data.clone()),
                    sheet_index: Some(*sheet_index), // 恢复到原始位置
                }
            }
            // SortColumn 的 undo：用排序前的数据恢复（不需要反向操作，因为已保存原始数据）
            Operation::SortColumn { sheet_index, col_index, ascending, old_sheet_data, previous_sort_state } => {
                // undo: 用 old_sheet_data 恢复排序前的状态
                // 返回一个新的 Operation，用 old_sheet_data 替换
                eprintln!("[undo method] creating undo op with previous_sort_state = None");
                Operation::SortColumn {
                    sheet_index: *sheet_index,
                    col_index: *col_index,
                    ascending: *ascending,
                    old_sheet_data: old_sheet_data.clone(),
                    previous_sort_state: previous_sort_state.clone(),
                }
            }
        }
    }
}
