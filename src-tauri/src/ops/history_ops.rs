use crate::ops::core_ops::Operation;
use crate::types::{FileData, OperationResult, SheetData};

impl Operation {
    /// 执行撤销操作
    pub fn undo(&self, file_data: &mut FileData) -> OperationResult {
        let undo_op = self.create_undo_op();
        undo_op.execute(file_data)
    }

    /// 创建撤销操作（返回反向操作）
    pub fn create_undo_op(&self) -> Operation {
        match self {
            Operation::SetCell {
                sheet_index,
                row,
                col,
                old_value,
                new_value,
            } => Operation::SetCell {
                sheet_index: *sheet_index,
                row: *row,
                col: *col,
                old_value: new_value.clone(),
                new_value: old_value.clone(),
            },
            Operation::AddRow {
                sheet_index,
                row_index,
                row_data,
            } => Operation::DeleteRow {
                sheet_index: *sheet_index,
                row_index: *row_index,
                row_data: row_data.clone(),
            },
            Operation::DeleteRow {
                sheet_index,
                row_index,
                row_data,
            } => Operation::AddRow {
                sheet_index: *sheet_index,
                row_index: *row_index,
                row_data: row_data.clone(),
            },
            Operation::AddColumn {
                sheet_index,
                col_index,
                col_data,
            } => Operation::DeleteColumn {
                sheet_index: *sheet_index,
                col_index: col_index.unwrap_or(0),
                col_data: col_data.clone(),
            },
            Operation::DeleteColumn {
                sheet_index,
                col_index,
                col_data,
            } => Operation::AddColumn {
                sheet_index: *sheet_index,
                col_index: Some(*col_index),
                col_data: col_data.clone(),
            },
            Operation::AddSheet { sheet_index, .. } => Operation::DeleteSheet {
                sheet_index: sheet_index.unwrap_or(usize::MAX),
                sheet_data: SheetData::default(),
            },
            Operation::DeleteSheet {
                sheet_index,
                sheet_data,
            } => Operation::AddSheet {
                name: sheet_data.name.clone(),
                sheet_data: Some(sheet_data.clone()),
                sheet_index: Some(*sheet_index),
            },
            Operation::SortColumn {
                sheet_index,
                col_index,
                ascending,
                old_sheet_data,
                previous_sort_state,
            } => Operation::SortColumn {
                sheet_index: *sheet_index,
                col_index: *col_index,
                ascending: *ascending,
                old_sheet_data: old_sheet_data.clone(),
                previous_sort_state: previous_sort_state.clone(),
            },
        }
    }

    /// 创建重做操作
    /// SortColumn 需要更新 old_sheet_data 为当前状态，用于 redo 时恢复排序后的状态
    pub fn create_redo_op(&self, file_data: &mut FileData) -> Operation {
        match self {
            Operation::SortColumn {
                sheet_index,
                col_index,
                ascending,
                old_sheet_data: _,
                previous_sort_state,
            } => {
                if let Some(sheet) = file_data.sheets.get(*sheet_index) {
                    Operation::SortColumn {
                        sheet_index: *sheet_index,
                        col_index: *col_index,
                        ascending: *ascending,
                        old_sheet_data: sheet.clone(),
                        previous_sort_state: previous_sort_state.clone(),
                    }
                } else {
                    self.clone()
                }
            }
            _ => self.clone(),
        }
    }
}
