use crate::ops::core_ops::Operation;
use crate::types::SheetData;

impl Operation {
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
                row_height,
            } => Operation::DeleteRow {
                sheet_index: *sheet_index,
                row_index: *row_index,
                row_data: row_data.clone(),
                row_height: *row_height,
            },
            Operation::DeleteRow {
                sheet_index,
                row_index,
                row_data,
                row_height,
            } => Operation::AddRow {
                sheet_index: *sheet_index,
                row_index: *row_index,
                row_data: row_data.clone(),
                row_height: *row_height,
            },
            Operation::AddColumn {
                sheet_index,
                col_index,
                col_data,
                column_width,
            } => Operation::DeleteColumn {
                sheet_index: *sheet_index,
                col_index: col_index.unwrap_or(0),
                col_data: col_data.clone(),
                column_width: *column_width,
            },
            Operation::DeleteColumn {
                sheet_index,
                col_index,
                col_data,
                column_width,
            } => Operation::AddColumn {
                sheet_index: *sheet_index,
                col_index: Some(*col_index),
                col_data: col_data.clone(),
                column_width: *column_width,
            },
            Operation::SetColumnWidth {
                sheet_index,
                col_index,
                old_width,
                new_width,
            } => Operation::SetColumnWidth {
                sheet_index: *sheet_index,
                col_index: *col_index,
                old_width: *new_width,
                new_width: *old_width,
            },
            Operation::SetRowHeight {
                sheet_index,
                row_index,
                old_height,
                new_height,
            } => Operation::SetRowHeight {
                sheet_index: *sheet_index,
                row_index: *row_index,
                old_height: *new_height,
                new_height: *old_height,
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
        }
    }

    /// 创建重做操作
    pub fn create_redo_op(&self) -> Operation {
        self.clone()
    }
}
