use crate::domain::{AppliedOperation, OperationPatchProjector};
use crate::types::{
    AppliedOperationResult, CellValue, ColumnChange, FileData, RowChange, SheetData,
};

impl OperationPatchProjector<'_> {
    pub fn projected_result_from_current_file(
        &self,
        file_data: &FileData,
    ) -> AppliedOperationResult {
        match self.operation {
            AppliedOperation::SetCell { .. }
            | AppliedOperation::SetCells { .. }
            | AppliedOperation::SetColumnWidth { .. }
            | AppliedOperation::SetRowHeight { .. } => {
                unreachable!("cell/layout operations already return from execute_cells_and_layout")
            }
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
