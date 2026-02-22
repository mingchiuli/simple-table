use serde::{Deserialize, Serialize};
use crate::types::{CellChange, CellValue, ColumnChange, FileData, OperationResult, RowChange};

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
    },
    /// 删除列
    DeleteColumn {
        sheet_index: usize,
        col_index: usize,
    },
}

impl Operation {
    /// 执行操作，返回增量结果
    pub fn execute(&self, file_data: &mut FileData) -> OperationResult {
        match self {
            Operation::SetCell { sheet_index, row, col, old_value: _, new_value } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    if let Some(row_data) = sheet.rows.get_mut(*row) {
                        if *col < row_data.len() {
                            row_data[*col] = new_value.clone();
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
                }
                OperationResult::DeleteRow {
                    sheet_index: *sheet_index,
                    row_index: *row_index,
                }
            }
            Operation::AddColumn { sheet_index } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    for row in &mut sheet.rows {
                        row.push(CellValue::Null);
                    }
                }
                let col_index = file_data.sheets
                    .get(*sheet_index)
                    .and_then(|s| s.rows.first())
                    .map(|r| r.len().saturating_sub(1))
                    .unwrap_or(0);
                OperationResult::AddColumn {
                    sheet_index: *sheet_index,
                    column: ColumnChange { index: col_index },
                }
            }
            Operation::DeleteColumn { sheet_index, col_index } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    for row in &mut sheet.rows {
                        if *col_index < row.len() {
                            row.remove(*col_index);
                        }
                    }
                }
                OperationResult::DeleteColumn {
                    sheet_index: *sheet_index,
                    column_index: *col_index,
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
                    row_data: vec![],
                }
            }
            Operation::DeleteRow { sheet_index, row_index, .. } => {
                Operation::AddRow {
                    sheet_index: *sheet_index,
                    row_index: *row_index,
                }
            }
            Operation::AddColumn { sheet_index } => {
                Operation::DeleteColumn {
                    sheet_index: *sheet_index,
                    col_index: 0,
                }
            }
            Operation::DeleteColumn { sheet_index, .. } => {
                Operation::AddColumn {
                    sheet_index: *sheet_index,
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
    pub fn execute(&mut self, operation: Operation) -> OperationResult {
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
