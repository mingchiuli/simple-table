use crate::formula::engine::FormulaRuntime;
use crate::ops::Operation;
use crate::state::content_hash::{ContentHash, hash_file_content};
use crate::types::{CellValue, FileData, OperationResult, SheetCellChange};

#[derive(Debug, Clone)]
pub struct ExecutedOperation {
    pub operation: OperationResult,
    pub cell_changes: Vec<SheetCellChange>,
}

/// 编辑器状态管理器
pub struct EditorState {
    pub file_data: FileData,
    pub history: Vec<Operation>,
    pub redo_stack: Vec<Operation>,
    pub can_undo: bool,
    pub can_redo: bool,
    pub current_content_hash: ContentHash,
    pub saved_content_hash: ContentHash,
    formula_runtime: FormulaRuntime,
}

impl EditorState {
    pub fn new(mut file_data: FileData) -> Self {
        let formula_runtime = FormulaRuntime::new(&mut file_data).unwrap_or_else(|error| {
            eprintln!("Formula runtime initialization failed: {error}");
            FormulaRuntime::empty()
        });
        let content_hash = hash_file_content(&file_data);
        Self {
            file_data,
            history: Vec::new(),
            redo_stack: Vec::new(),
            can_undo: false,
            can_redo: false,
            current_content_hash: content_hash,
            saved_content_hash: content_hash,
            formula_runtime,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.current_content_hash != self.saved_content_hash
    }

    pub fn mark_saved(&mut self) {
        self.refresh_content_hash();
        self.saved_content_hash = self.current_content_hash;
    }

    /// 执行操作并记录到历史，返回增量结果
    pub fn execute(&mut self, mut operation: Operation) -> ExecutedOperation {
        // 在执行操作前，先准备好需要的数据，以便撤销/重做
        match &operation {
            // SetCell: 从 file_data 中获取真正的旧值，而不是依赖前端传入的（可能已过时）
            Operation::SetCell {
                sheet_index,
                row,
                col,
                old_value,
                new_value,
            } => {
                if let Some(sheet) = self.file_data.sheets.get(*sheet_index)
                    && let Some(real_old) = sheet.rows.get(*row).and_then(|r| r.get(*col))
                {
                    // 如果新值和旧值相同，不需要记录到 history
                    if real_old == new_value {
                        // 返回结果但不记录到 history
                        let result = operation.execute(&mut self.file_data);
                        let cell_changes = self.recalculate_after_operation(&operation);
                        self.update_flags();
                        self.refresh_content_hash();
                        return ExecutedOperation {
                            operation: result,
                            cell_changes,
                        };
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
            // AddColumn: 添加空列，需要补充列索引（只当 col_data 为空时）
            Operation::AddColumn {
                sheet_index,
                col_index,
                col_data: _,
            } => {
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
            Operation::AddRow {
                sheet_index,
                row_index,
                row_data,
            } => {
                // 如果 row_data 已有数据（撤销操作），保留原数据
                if row_data.is_empty()
                    && *sheet_index < self.file_data.sheets.len()
                    && let Some(sheet) = self.file_data.sheets.get(*sheet_index)
                {
                    let col_count = sheet.rows.first().map(|r| r.len()).unwrap_or(0);
                    operation = Operation::AddRow {
                        sheet_index: *sheet_index,
                        row_index: *row_index,
                        row_data: vec![CellValue::Null; col_count],
                    };
                }
            }
            Operation::DeleteSheet {
                sheet_index,
                sheet_data,
            } => {
                // 如果 sheet_data 为空，说明是正常的删除操作，需要保存完整的 sheet 数据
                if sheet_data.is_empty()
                    && *sheet_index < self.file_data.sheets.len()
                    && let Some(removed_sheet) = self.file_data.sheets.get(*sheet_index)
                {
                    operation = Operation::DeleteSheet {
                        sheet_index: *sheet_index,
                        sheet_data: removed_sheet.clone(),
                    };
                }
            }
            // AddSheet: 提前生成名称和索引，保证 redo 重放时使用与首次执行一致的位置和名称
            Operation::AddSheet {
                name,
                sheet_data,
                sheet_index,
            } => {
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
        let cell_changes = self.recalculate_after_operation(&operation);
        self.history.push(operation);
        self.redo_stack.clear();
        self.update_flags();
        self.refresh_content_hash();
        ExecutedOperation {
            operation: result,
            cell_changes,
        }
    }

    /// 撤销上一个操作
    pub fn undo(&mut self) -> Option<ExecutedOperation> {
        if let Some(operation) = self.history.pop() {
            // 执行 undo 操作
            let result = operation.undo(&mut self.file_data);
            self.rebuild_formula_runtime();
            // 获取 redo 操作
            let redo_op = operation.create_redo_op(&mut self.file_data);
            self.redo_stack.push(redo_op);

            self.update_flags();
            self.refresh_content_hash();
            Some(ExecutedOperation {
                operation: result,
                cell_changes: Vec::new(),
            })
        } else {
            None
        }
    }

    /// 重做上一个被撤销的操作
    pub fn redo(&mut self) -> Option<ExecutedOperation> {
        if let Some(operation) = self.redo_stack.pop() {
            let result = operation.execute(&mut self.file_data);
            let cell_changes = self.recalculate_after_operation(&operation);
            self.history.push(operation);
            self.update_flags();
            self.refresh_content_hash();
            Some(ExecutedOperation {
                operation: result,
                cell_changes,
            })
        } else {
            None
        }
    }

    fn update_flags(&mut self) {
        self.can_undo = !self.history.is_empty();
        self.can_redo = !self.redo_stack.is_empty();
    }

    fn refresh_content_hash(&mut self) {
        self.current_content_hash = hash_file_content(&self.file_data);
    }

    fn recalculate_after_operation(&mut self, operation: &Operation) -> Vec<SheetCellChange> {
        match operation {
            Operation::SetCell {
                sheet_index,
                row,
                col,
                new_value,
                ..
            } => {
                let result = self.formula_runtime.sync_cell_and_recalculate(
                    &mut self.file_data,
                    *sheet_index,
                    *row,
                    *col,
                );

                match result {
                    Ok(changes) => changes,
                    Err(error) => {
                        eprintln!("Formula recalculation failed: {error}");
                        let changes = self.formula_error_change(
                            *sheet_index,
                            *row,
                            *col,
                            new_value,
                            error.to_string(),
                        );
                        self.rebuild_formula_runtime();
                        changes
                    }
                }
            }
            _ => match self.formula_runtime.rebuild(&mut self.file_data) {
                Ok(()) => Vec::new(),
                Err(error) => {
                    eprintln!("Formula recalculation failed: {error}");
                    self.rebuild_formula_runtime();
                    Vec::new()
                }
            },
        }
    }

    fn formula_error_change(
        &mut self,
        sheet_index: usize,
        row: usize,
        col: usize,
        value: &CellValue,
        error: String,
    ) -> Vec<SheetCellChange> {
        if !matches!(value, CellValue::Formula { .. }) {
            return Vec::new();
        }

        let Some(cell) = self
            .file_data
            .sheets
            .get_mut(sheet_index)
            .and_then(|sheet| sheet.rows.get_mut(row))
            .and_then(|row_data| row_data.get_mut(col))
        else {
            return Vec::new();
        };

        *cell = cell.with_formula_result(CellValue::Null, Some(error));
        vec![SheetCellChange {
            sheet_index,
            row,
            col,
            value: cell.clone(),
        }]
    }

    fn rebuild_formula_runtime(&mut self) {
        if let Err(error) = self.formula_runtime.rebuild(&mut self.file_data) {
            eprintln!("Formula runtime rebuild failed: {error}");
            self.formula_runtime = FormulaRuntime::empty();
        }
    }
}
