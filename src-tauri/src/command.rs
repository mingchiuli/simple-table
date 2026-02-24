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
pub fn rebuild_sheet_index(sheet: &mut crate::types::SheetData) {
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
    },
    /// 删除列
    DeleteColumn {
        sheet_index: usize,
        col_index: usize,
        col_data: Vec<CellValue>,
    },
    /// 添加 Sheet
    AddSheet,
    /// 删除 Sheet
    DeleteSheet {
        sheet_index: usize,
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
            Operation::AddColumn { sheet_index } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    for row in &mut sheet.rows {
                        row.push(CellValue::Null);
                    }
                    // 索引重建由调用方异步处理
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
            Operation::AddSheet => {
                // 生成新 sheet 名称
                let sheet_count = file_data.sheets.len();
                let new_sheet_name = format!("Sheet{}", sheet_count + 1);

                // 创建新的空 sheet
                let new_sheet = SheetData {
                    name: new_sheet_name.clone(),
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

                let new_sheet_index = file_data.sheets.len();
                file_data.sheets.push(new_sheet);

                OperationResult::AddSheet {
                    sheet_index: new_sheet_index,
                    name: new_sheet_name,
                }
            }
            Operation::DeleteSheet { sheet_index } => {
                // Don't allow deleting the last sheet
                if file_data.sheets.len() <= 1 {
                    return OperationResult::AddSheet {
                        sheet_index: 0,
                        name: "Error".to_string(),
                    };
                }

                let _removed_sheet = file_data.sheets.remove(*sheet_index);

                // Adjust current sheet index if needed
                let new_current_index = if *sheet_index >= file_data.sheets.len() {
                    file_data.sheets.len() - 1
                } else {
                    *sheet_index
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
                    col_data: vec![],
                }
            }
            Operation::DeleteColumn { sheet_index, .. } => {
                Operation::AddColumn {
                    sheet_index: *sheet_index,
                }
            }
            Operation::AddSheet => {
                // Undo for add sheet not supported in this simplified version
                Operation::AddSheet
            }
            Operation::DeleteSheet { .. } => {
                // Undo for delete sheet not supported in this simplified version
                Operation::AddSheet
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
