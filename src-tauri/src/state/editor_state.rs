use serde::{Deserialize, Serialize};
use crate::types::{CellValue, FileData, OperationResult};
pub use crate::ops::undo_ops::{Operation, Undoable};

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
            // AddColumn: 添加空列，需要补充列索引（只当 col_data 为空时）
            Operation::AddColumn { sheet_index, col_index, col_data: _ } => {
                // 如果 col_data 已有数据（撤销操作），保留原数据
                if col_index.is_none() && *sheet_index < self.file_data.sheets.len() {
                    // 正常添加列，补充列索引
                    if let Some(sheet) = self.file_data.sheets.get(*sheet_index) {
                        let col_count = sheet.rows.first().map(|r| r.len()).unwrap_or(0);
                        // 新列被添加到末尾，索引就是当前列数（添加前）
                        operation = Operation::AddColumn {
                            sheet_index: *sheet_index,
                            col_index: Some(col_count),
                            col_data: vec![],
                        };
                    }
                }
            }
            // AddRow: 添加空行，需要补充行数据（只当 row_data 为空时）
            Operation::AddRow { sheet_index, row_index, row_data } => {
                // 如果 row_data 已有数据（撤销操作），保留原数据
                if row_data.is_empty() && *sheet_index < self.file_data.sheets.len() {
                    if let Some(sheet) = self.file_data.sheets.get(*sheet_index) {
                        let col_count = sheet.rows.first().map(|r| r.len()).unwrap_or(0);
                        operation = Operation::AddRow {
                            sheet_index: *sheet_index,
                            row_index: *row_index,
                            row_data: vec![CellValue::Null; col_count],
                        };
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
            // AddSheet: 提前生成名称和索引，保证 redo 重放时使用与首次执行一致的位置和名称
            Operation::AddSheet { name, sheet_data, sheet_index } => {
                if sheet_data.is_none() {
                    let final_name = if name.is_empty() {
                        format!("Sheet{}", self.file_data.sheets.len() + 1)
                    } else {
                        name.clone()
                    };
                    let actual_index = sheet_index.unwrap_or(self.file_data.sheets.len());
                    operation = Operation::AddSheet {
                        name: final_name,
                        sheet_data: None,
                        sheet_index: Some(actual_index),
                    };
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
            // 执行 undo 操作
            let result = operation.undo(&mut self.file_data);
            // 获取 redo 操作（使用 trait 方法，让操作自己决定 redo 行为）
            let redo_op = operation.get_redo_operation(&mut self.file_data);
            self.redo_stack.push(redo_op);

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
}
