use crate::domain::{AppliedOperation, OperationPatchProjector};
use crate::types::{
    AppliedOperationResult, CellChange, CellValue, ColumnChange, ColumnWidthChange, FileData,
    RowChange, RowHeightChange, SheetCellChange, SheetData,
};

impl OperationPatchProjector<'_> {
    pub fn projected_result_from_current_file(
        &self,
        file_data: &FileData,
    ) -> AppliedOperationResult {
        match self.operation {
            AppliedOperation::SetCell {
                sheet_index,
                row,
                col,
                new_value,
                ..
            } => AppliedOperationResult::SetCell {
                sheet_index: *sheet_index,
                cell: CellChange {
                    row: *row,
                    col: *col,
                    value: file_data
                        .sheets
                        .get(*sheet_index)
                        .and_then(|sheet| sheet.rows.get(*row))
                        .and_then(|row_data| row_data.get(*col))
                        .cloned()
                        .unwrap_or_else(|| new_value.clone()),
                },
            },
            AppliedOperation::SetCells { changes } => AppliedOperationResult::SetCells {
                changes: changes
                    .iter()
                    .map(|change| {
                        SheetCellChange::new(
                            change.sheet_index,
                            change.row,
                            change.col,
                            file_data
                                .sheets
                                .get(change.sheet_index)
                                .and_then(|sheet| sheet.rows.get(change.row))
                                .and_then(|row| row.get(change.col))
                                .cloned()
                                .unwrap_or_else(|| change.new_value.clone()),
                        )
                    })
                    .collect(),
            },
            AppliedOperation::SetColumnWidth {
                sheet_index,
                col_index,
                new_width,
                ..
            } => AppliedOperationResult::SetColumnWidth {
                sheet_index: *sheet_index,
                column: ColumnWidthChange {
                    col_index: *col_index,
                    width: *new_width,
                },
            },
            AppliedOperation::SetRowHeight {
                sheet_index,
                row_index,
                new_height,
                ..
            } => AppliedOperationResult::SetRowHeight {
                sheet_index: *sheet_index,
                row: RowHeightChange {
                    row_index: *row_index,
                    height: *new_height,
                },
            },
            AppliedOperation::AddRow {
                sheet_index,
                row_index,
                ..
            } => AppliedOperationResult::AddRow {
                sheet_index: *sheet_index,
                row: RowChange {
                    index: *row_index,
                    values: file_data
                        .sheets
                        .get(*sheet_index)
                        .and_then(|sheet| sheet.rows.get(*row_index))
                        .cloned()
                        .unwrap_or_default(),
                },
            },
            AppliedOperation::DeleteRow {
                sheet_index,
                row_index,
            } => AppliedOperationResult::DeleteRow {
                sheet_index: *sheet_index,
                row_index: *row_index,
            },
            AppliedOperation::AddColumn {
                sheet_index,
                col_index,
                ..
            } => AppliedOperationResult::AddColumn {
                sheet_index: *sheet_index,
                column: ColumnChange { index: *col_index },
                col_data: file_data
                    .sheets
                    .get(*sheet_index)
                    .map(|sheet| {
                        sheet
                            .rows
                            .iter()
                            .map(|row| row.get(*col_index).cloned().unwrap_or(CellValue::Null))
                            .collect()
                    })
                    .unwrap_or_default(),
            },
            AppliedOperation::DeleteColumn {
                sheet_index,
                col_index,
            } => AppliedOperationResult::DeleteColumn {
                sheet_index: *sheet_index,
                column_index: *col_index,
            },
            AppliedOperation::AddSheet {
                sheet_index,
                name,
                row_count,
                column_count,
            } => AppliedOperationResult::AddSheet {
                sheet_index: *sheet_index,
                name: name.clone(),
                sheet_data: file_data
                    .sheets
                    .get(*sheet_index)
                    .cloned()
                    .unwrap_or_else(|| SheetData {
                        name: name.clone(),
                        rows: vec![vec![CellValue::Null; *column_count]; *row_count],
                        ..Default::default()
                    }),
            },
            AppliedOperation::DeleteSheet { sheet_index } => AppliedOperationResult::DeleteSheet {
                sheet_index: *sheet_index,
                sheet_data: SheetData::default(),
            },
        }
    }
}
