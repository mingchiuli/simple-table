use serde::{Deserialize, Serialize};
use crate::types::{CellValue, FileData, OperationResult};
pub use crate::ops::operation::Operation;

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
    pub fn execute(&mut self, mut operation: Operation) {
        // 在执行操作前，先准备好需要的数据，以便撤销/重做
        match &operation {
            // SetCell: 从 file_data 中获取真正的旧值，而不是依赖前端传入的（可能已过时）
            Operation::SetCell { sheet_index, row, col, old_value, new_value } => {
                if let Some(sheet) = self.file_data.sheets.get(*sheet_index) {
                    if let Some(real_old) = sheet.rows.get(*row).and_then(|r| r.get(*col)) {
                        // 如果新值和旧值相同，不需要记录到 history
                        if real_old == new_value {
                            // 返回结果但不记录到 history
                            operation.execute(&mut self.file_data);
                            self.update_flags();
                            return;
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

        operation.execute(&mut self.file_data);
        self.history.push(operation);
        self.redo_stack.clear();
        self.update_flags();
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
