use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::types::{CellChange, CellPosition, CellValue, ColumnChange, FileData, OperationResult, RowChange, SheetData, SheetIndex};

/// 将单元格值转换为字符串
fn cell_to_string(cell: &CellValue) -> String {
    match cell {
        CellValue::Null => String::new(),
        CellValue::String(s) => s.clone(),
        CellValue::Number(n) => n.to_string(),
        CellValue::Boolean(b) => b.to_string(),
    }
}

/// 重建单个 sheet 的索引（公开给 tauri_commands 调用）
pub fn rebuild_sheet_index(sheet: &mut SheetData) {
    let mut inverted_index: HashMap<String, Vec<CellPosition>> = HashMap::new();

    for (row_idx, row) in sheet.rows.iter().enumerate() {
        for (col_idx, cell) in row.iter().enumerate() {
            let text = cell_to_string(cell);
            if !text.is_empty() {
                let token = text.to_lowercase();
                inverted_index
                    .entry(token)
                    .or_default()
                    .push(CellPosition {
                        row: row_idx,
                        col: col_idx,
                    });
            }
        }
    }

    sheet.index.inverted_index = inverted_index;
}

/// 更新单个单元格的索引
fn update_cell_index(sheet: &mut crate::types::SheetData, row: usize, col: usize, old_value: &CellValue, new_value: &CellValue) {
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
    },
    /// 删除 Sheet（带完整数据，用于撤销时恢复）
    DeleteSheet {
        sheet_index: usize,
        sheet_data: SheetData,
    },
}

impl Operation {
    /// 执行操作，返回增量结果
    /// 注意：此方法不再同步重建索引，索引重建由调用方异步处理
    pub fn execute(&self, file_data: &mut FileData) -> OperationResult {
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
            Operation::AddRow { sheet_index, row_index } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    let col_count = sheet.rows.first().map(|r| r.len()).unwrap_or(0);
                    let new_row = vec![CellValue::Null; col_count];
                    sheet.rows.insert(*row_index, new_row);
                    // 索引重建由调用方异步处理
                }
                OperationResult::AddRow {
                    sheet_index: *sheet_index,
                    row: RowChange {
                        index: *row_index,
                        values: vec![],
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
            Operation::AddColumn { sheet_index, col_index, .. } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    for row in &mut sheet.rows {
                        row.push(CellValue::Null);
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
            Operation::AddSheet { name, sheet_data } => {
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
                    };
                    (new_sheet, final_name)
                };

                let new_sheet_index = file_data.sheets.len();
                file_data.sheets.push(new_sheet);

                OperationResult::AddSheet {
                    sheet_index: new_sheet_index,
                    name: sheet_name,
                }
            }
            Operation::DeleteSheet { sheet_index, sheet_data } => {
                // Don't allow deleting the last sheet (仅对正常删除操作)
                if file_data.sheets.len() <= 1 && sheet_data.is_empty() {
                    return OperationResult::AddSheet {
                        sheet_index: 0,
                        name: "Error".to_string(),
                    };
                }

                // 如果 sheet_index 是 MAX，说明这是 AddSheet 的撤销操作，需要删除最后一个 sheet
                let actual_index = if *sheet_index == usize::MAX {
                    file_data.sheets.len().saturating_sub(1)
                } else {
                    *sheet_index
                };

                let _removed_sheet = file_data.sheets.remove(actual_index);

                // Adjust current sheet index if needed
                let new_current_index = if actual_index >= file_data.sheets.len() {
                    file_data.sheets.len().saturating_sub(1)
                } else {
                    actual_index
                };

                OperationResult::DeleteSheet {
                    sheet_index: new_current_index,
                }
            }
        }
    }

    /// 撤销操作（返回反向操作）
    pub fn undo(&self) -> Operation {
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
            Operation::AddRow { sheet_index, row_index } => {
                Operation::DeleteRow {
                    sheet_index: *sheet_index,
                    row_index: *row_index,
                    row_data: vec![], // AddRow 没有原始数据，DeleteRow 执行时从 file_data 获取
                }
            }
            Operation::DeleteRow { sheet_index, row_index, .. } => {
                Operation::AddRow {
                    sheet_index: *sheet_index,
                    row_index: *row_index,
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
            Operation::DeleteSheet { sheet_index: _, sheet_data } => {
                // DeleteSheet 的撤销：恢复被删除的 sheet（使用保存的完整数据）
                Operation::AddSheet {
                    name: sheet_data.name.clone(),
                    sheet_data: Some(sheet_data.clone()),
                }
            }
        }
    }
}

/// 编辑器状态管理器
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorState {
    pub file_data: FileData,
    #[serde(skip)]
    pub history: Vec<Operation>,
    #[serde(skip)]
    pub redo_stack: Vec<Operation>,
    pub can_undo: bool,
    pub can_redo: bool,
}

impl EditorState {
    pub fn new(file_data: FileData) -> Self {
        Self {
            file_data,
            history: Vec::new(),
            redo_stack: Vec::new(),
            can_undo: false,
            can_redo: false,
        }
    }

    /// 执行操作并记录到历史，返回增量结果
    pub fn execute(&mut self, mut operation: Operation) -> OperationResult {
        // 在执行操作前，先准备好需要的数据，以便撤销/重做
        match &operation {
            // SetCell: 从 file_data 中获取真正的旧值，而不是依赖前端传入的（可能已过时）
            Operation::SetCell { sheet_index, row, col, old_value, new_value } => {
                if let Some(sheet) = self.file_data.sheets.get(*sheet_index) {
                    if let Some(real_old) = sheet.rows.get(*row).and_then(|r| r.get(*col)) {
                        // 如果新值和旧值相同，不需要记录到 history
                        if real_old == new_value {
                            // 返回结果但不记录到 history
                            let result = operation.execute(&mut self.file_data);
                            self.update_flags();
                            return result;
                        }
                        // 只有当后端获取的旧值与前端传入的不同时，才更新 operation
                        if real_old != old_value {
                            operation = Operation::SetCell {
                                sheet_index: *sheet_index,
                                row: *row,
                                col: *col,
                                old_value: real_old.clone(),
                                new_value: new_value.clone(),
                            };
                        }
                    }
                }
            }
            Operation::DeleteRow { sheet_index, row_index, row_data } => {
                if row_data.is_empty() && *sheet_index < self.file_data.sheets.len() {
                    if let Some(sheet) = self.file_data.sheets.get(*sheet_index) {
                        if let Some(deleted_row) = sheet.rows.get(*row_index) {
                            operation = Operation::DeleteRow {
                                sheet_index: *sheet_index,
                                row_index: *row_index,
                                row_data: deleted_row.clone(),
                            };
                        }
                    }
                }
            }
            Operation::DeleteColumn { sheet_index, col_index, col_data } => {
                if col_data.is_empty() && *sheet_index < self.file_data.sheets.len() {
                    if let Some(sheet) = self.file_data.sheets.get(*sheet_index) {
                        let deleted_col: Vec<CellValue> = sheet.rows
                            .iter()
                            .map(|row| row.get(*col_index).cloned().unwrap_or(CellValue::Null))
                            .collect();
                        operation = Operation::DeleteColumn {
                            sheet_index: *sheet_index,
                            col_index: *col_index,
                            col_data: deleted_col,
                        };
                    }
                }
            }
            Operation::AddColumn { sheet_index, col_index, .. } => {
                // AddColumn 添加列到末尾，需要记录正确的列索引和列数据用于撤销
                if col_index.is_none() && *sheet_index < self.file_data.sheets.len() {
                    if let Some(sheet) = self.file_data.sheets.get(*sheet_index) {
                        let col_count = sheet.rows.first().map(|r| r.len()).unwrap_or(0);
                        if col_count > 0 {
                            // 获取添加的列数据（全是 Null）
                            let added_col: Vec<CellValue> = sheet.rows
                                .iter()
                                .map(|row| row.last().cloned().unwrap_or(CellValue::Null))
                                .collect();
                            operation = Operation::AddColumn {
                                sheet_index: *sheet_index,
                                col_index: Some(col_count - 1), // 记录添加的列索引
                                col_data: added_col,
                            };
                        }
                    }
                }
            }
            Operation::DeleteSheet { sheet_index, sheet_data } => {
                // 如果 sheet_data 为空，说明是正常的删除操作，需要保存完整的 sheet 数据
                if sheet_data.is_empty() && *sheet_index < self.file_data.sheets.len() {
                    if let Some(removed_sheet) = self.file_data.sheets.get(*sheet_index) {
                        operation = Operation::DeleteSheet {
                            sheet_index: *sheet_index,
                            sheet_data: removed_sheet.clone(),
                        };
                    }
                }
            }
            _ => {}
        }

        let result = operation.execute(&mut self.file_data);
        self.history.push(operation);
        self.redo_stack.clear();
        self.update_flags();
        result
    }

    /// 撤销上一个操作
    pub fn undo(&mut self) -> Option<OperationResult> {
        if let Some(operation) = self.history.pop() {
            // 保存原始操作到 redo_stack，以便 redo 时能恢复
            let original_op = operation.clone();
            let undo_op = operation.undo();
            let result = undo_op.execute(&mut self.file_data);
            self.redo_stack.push(original_op);
            self.update_flags();
            Some(result)
        } else {
            None
        }
    }

    /// 重做上一个被撤销的操作
    pub fn redo(&mut self) -> Option<OperationResult> {
        if let Some(operation) = self.redo_stack.pop() {
            let result = operation.execute(&mut self.file_data);
            self.history.push(operation);
            self.update_flags();
            Some(result)
        } else {
            None
        }
    }

    fn update_flags(&mut self) {
        self.can_undo = !self.history.is_empty();
        self.can_redo = !self.redo_stack.is_empty();
    }

    #[allow(dead_code)]
    /// 获取文件数据
    pub fn get_file_data(&self) -> &FileData {
        &self.file_data
    }
}
