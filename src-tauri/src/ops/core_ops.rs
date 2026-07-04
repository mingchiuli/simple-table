use crate::error::AppError;
use crate::types::{
    AppliedOperationResult, CellChange, CellValue, ColumnChange, ColumnWidthChange, FileData,
    MergeRange, RowChange, RowHeightChange, SetCellRequest, SheetCellChange, SheetData,
    parse_cell_text,
};
use serde::{Deserialize, Serialize};

/// User-facing editor command.
///
/// This is intentionally not a history record. Undo/redo is handled by
/// document mementos, so command variants only describe the requested mutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EditorCommand {
    SetCell {
        sheet_index: usize,
        row: usize,
        col: usize,
        text: String,
    },
    SetCells {
        changes: Vec<SetCellRequest>,
    },
    AddRow {
        sheet_index: usize,
        row_index: usize,
    },
    DeleteRow {
        sheet_index: usize,
        row_index: usize,
    },
    AddColumn {
        sheet_index: usize,
    },
    DeleteColumn {
        sheet_index: usize,
        col_index: usize,
    },
    SetColumnWidth {
        sheet_index: usize,
        col_index: usize,
        width: Option<u32>,
    },
    SetRowHeight {
        sheet_index: usize,
        row_index: usize,
        height: Option<u32>,
    },
    AddSheet {
        name: Option<String>,
    },
    DeleteSheet {
        sheet_index: usize,
    },
}

/// Canonical mutation after resolving indices and current document state.
#[derive(Debug, Clone)]
pub enum AppliedOperation {
    SetCell {
        sheet_index: usize,
        row: usize,
        col: usize,
        old_value: CellValue,
        new_value: CellValue,
    },
    SetCells {
        changes: Vec<ResolvedCellEdit>,
    },
    AddRow {
        sheet_index: usize,
        row_index: usize,
        row_data: Vec<CellValue>,
        row_height: Option<u32>,
    },
    DeleteRow {
        sheet_index: usize,
        row_index: usize,
    },
    AddColumn {
        sheet_index: usize,
        col_index: usize,
        col_data: Vec<CellValue>,
        column_width: Option<u32>,
    },
    DeleteColumn {
        sheet_index: usize,
        col_index: usize,
    },
    SetColumnWidth {
        sheet_index: usize,
        col_index: usize,
        old_width: Option<u32>,
        new_width: Option<u32>,
    },
    SetRowHeight {
        sheet_index: usize,
        row_index: usize,
        old_height: Option<u32>,
        new_height: Option<u32>,
    },
    AddSheet {
        sheet_index: usize,
        sheet_data: SheetData,
    },
    DeleteSheet {
        sheet_index: usize,
    },
}

#[derive(Debug, Clone)]
pub struct ResolvedCellEdit {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub old_value: CellValue,
    pub new_value: CellValue,
}

pub struct ProjectionMutation<'a> {
    operation: &'a AppliedOperation,
}

pub struct OperationPatchProjector<'a> {
    operation: &'a AppliedOperation,
}

pub struct MutationImpact<'a> {
    operation: &'a AppliedOperation,
}

impl EditorCommand {
    pub fn resolve(self, file_data: &FileData) -> Result<AppliedOperation, AppError> {
        match self {
            EditorCommand::SetCell {
                sheet_index,
                row,
                col,
                text,
            } => {
                require_sheet(file_data, sheet_index)?;
                let old_value = file_data.sheets[sheet_index]
                    .rows
                    .get(row)
                    .and_then(|row_data| row_data.get(col))
                    .cloned()
                    .unwrap_or(CellValue::Null);
                Ok(AppliedOperation::SetCell {
                    sheet_index,
                    row,
                    col,
                    old_value,
                    new_value: parse_cell_text(&text),
                })
            }
            EditorCommand::SetCells { changes } => {
                if changes.is_empty() {
                    return Ok(AppliedOperation::SetCells {
                        changes: Vec::new(),
                    });
                }
                let mut resolved = Vec::with_capacity(changes.len());
                for change in changes {
                    require_sheet(file_data, change.sheet_index)?;
                    let old_value = file_data.sheets[change.sheet_index]
                        .rows
                        .get(change.row)
                        .and_then(|row_data| row_data.get(change.col))
                        .cloned()
                        .unwrap_or(CellValue::Null);
                    resolved.push(ResolvedCellEdit {
                        sheet_index: change.sheet_index,
                        row: change.row,
                        col: change.col,
                        old_value,
                        new_value: parse_cell_text(&change.text),
                    });
                }
                Ok(AppliedOperation::SetCells { changes: resolved })
            }
            EditorCommand::AddRow {
                sheet_index,
                row_index,
            } => {
                let sheet = require_sheet(file_data, sheet_index)?;
                if row_index > sheet.rows.len() {
                    return Err(AppError::RowNotFound(row_index));
                }
                let col_count = sheet.rows.iter().map(Vec::len).max().unwrap_or(0);
                Ok(AppliedOperation::AddRow {
                    sheet_index,
                    row_index,
                    row_data: vec![CellValue::Null; col_count],
                    row_height: None,
                })
            }
            EditorCommand::DeleteRow {
                sheet_index,
                row_index,
            } => {
                let sheet = require_sheet(file_data, sheet_index)?;
                if row_index >= sheet_row_extent(sheet) {
                    return Err(AppError::RowNotFound(row_index));
                }
                Ok(AppliedOperation::DeleteRow {
                    sheet_index,
                    row_index,
                })
            }
            EditorCommand::AddColumn { sheet_index } => {
                let sheet = require_sheet(file_data, sheet_index)?;
                let col_index = sheet_column_extent(sheet);
                Ok(AppliedOperation::AddColumn {
                    sheet_index,
                    col_index,
                    col_data: vec![CellValue::Null; sheet.rows.len()],
                    column_width: None,
                })
            }
            EditorCommand::DeleteColumn {
                sheet_index,
                col_index,
            } => {
                let sheet = require_sheet(file_data, sheet_index)?;
                let total_cols = sheet_column_extent(sheet);
                if col_index >= total_cols {
                    return Err(AppError::InvalidCellPosition {
                        row: 0,
                        col: col_index,
                    });
                }
                Ok(AppliedOperation::DeleteColumn {
                    sheet_index,
                    col_index,
                })
            }
            EditorCommand::SetColumnWidth {
                sheet_index,
                col_index,
                width,
            } => {
                let sheet = require_sheet(file_data, sheet_index)?;
                let old_width = sheet
                    .column_widths
                    .as_ref()
                    .and_then(|widths| widths.get(&col_index).copied());
                Ok(AppliedOperation::SetColumnWidth {
                    sheet_index,
                    col_index,
                    old_width,
                    new_width: width,
                })
            }
            EditorCommand::SetRowHeight {
                sheet_index,
                row_index,
                height,
            } => {
                let sheet = require_sheet(file_data, sheet_index)?;
                let old_height = sheet
                    .row_heights
                    .as_ref()
                    .and_then(|heights| heights.get(&row_index).copied());
                Ok(AppliedOperation::SetRowHeight {
                    sheet_index,
                    row_index,
                    old_height,
                    new_height: height,
                })
            }
            EditorCommand::AddSheet { name } => {
                let sheet_index = file_data.sheets.len();
                let sheet_name = name
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| format!("Sheet{}", sheet_index + 1));
                Ok(AppliedOperation::AddSheet {
                    sheet_index,
                    sheet_data: empty_sheet(sheet_name),
                })
            }
            EditorCommand::DeleteSheet { sheet_index } => {
                if file_data.sheets.len() <= 1 {
                    return Err(AppError::CannotDeleteLastSheet);
                }
                require_sheet(file_data, sheet_index)?;
                Ok(AppliedOperation::DeleteSheet { sheet_index })
            }
        }
    }
}

impl AppliedOperation {
    pub fn projection_mutation(&self) -> ProjectionMutation<'_> {
        ProjectionMutation { operation: self }
    }

    pub fn patch_projector(&self) -> OperationPatchProjector<'_> {
        OperationPatchProjector { operation: self }
    }

    pub fn impact(&self) -> MutationImpact<'_> {
        MutationImpact { operation: self }
    }
}

impl ProjectionMutation<'_> {
    pub fn execute(&self, file_data: &mut FileData) -> AppliedOperationResult {
        match self.operation {
            AppliedOperation::SetCell {
                sheet_index,
                row,
                col,
                new_value,
                ..
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    ensure_cell_exists(sheet, *row, *col);
                    sheet.rows[*row][*col] = new_value.clone();
                }
                AppliedOperationResult::SetCell {
                    sheet_index: *sheet_index,
                    cell: CellChange {
                        row: *row,
                        col: *col,
                        value: new_value.clone(),
                    },
                }
            }
            AppliedOperation::SetCells { changes } => {
                for change in changes {
                    if let Some(sheet) = file_data.sheets.get_mut(change.sheet_index) {
                        ensure_cell_exists(sheet, change.row, change.col);
                        sheet.rows[change.row][change.col] = change.new_value.clone();
                    }
                }
                AppliedOperationResult::SetCells {
                    changes: changes
                        .iter()
                        .map(|change| {
                            SheetCellChange::new(
                                change.sheet_index,
                                change.row,
                                change.col,
                                change.new_value.clone(),
                            )
                        })
                        .collect(),
                }
            }
            AppliedOperation::AddRow {
                sheet_index,
                row_index,
                row_data,
                row_height,
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    sheet.rows.insert(*row_index, row_data.clone());
                    shift_layout_map_on_insert(sheet.row_heights.as_mut(), *row_index);
                    shift_row_merges_on_insert(&mut sheet.merges, *row_index);
                    if let Some(height) = row_height {
                        sheet
                            .row_heights
                            .get_or_insert_with(Default::default)
                            .insert(*row_index, *height);
                    }
                }
                AppliedOperationResult::AddRow {
                    sheet_index: *sheet_index,
                    row: RowChange {
                        index: *row_index,
                        values: row_data.clone(),
                    },
                }
            }
            AppliedOperation::DeleteRow {
                sheet_index,
                row_index,
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    if *row_index < sheet.rows.len() {
                        sheet.rows.remove(*row_index);
                    }
                    shift_layout_map_on_delete(sheet.row_heights.as_mut(), *row_index);
                    shift_row_merges_on_delete(&mut sheet.merges, *row_index);
                }
                AppliedOperationResult::DeleteRow {
                    sheet_index: *sheet_index,
                    row_index: *row_index,
                }
            }
            AppliedOperation::AddColumn {
                sheet_index,
                col_index,
                col_data,
                column_width,
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    for (row_index, row) in sheet.rows.iter_mut().enumerate() {
                        let value = col_data.get(row_index).cloned().unwrap_or(CellValue::Null);
                        let pos = (*col_index).min(row.len());
                        row.insert(pos, value);
                    }
                    shift_layout_map_on_insert(sheet.column_widths.as_mut(), *col_index);
                    shift_column_merges_on_insert(&mut sheet.merges, *col_index);
                    if let Some(width) = column_width {
                        sheet
                            .column_widths
                            .get_or_insert_with(Default::default)
                            .insert(*col_index, *width);
                    }
                }
                AppliedOperationResult::AddColumn {
                    sheet_index: *sheet_index,
                    column: ColumnChange { index: *col_index },
                    col_data: col_data.clone(),
                }
            }
            AppliedOperation::DeleteColumn {
                sheet_index,
                col_index,
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    for row in &mut sheet.rows {
                        if *col_index < row.len() {
                            row.remove(*col_index);
                        }
                    }
                    shift_layout_map_on_delete(sheet.column_widths.as_mut(), *col_index);
                    shift_column_merges_on_delete(&mut sheet.merges, *col_index);
                }
                AppliedOperationResult::DeleteColumn {
                    sheet_index: *sheet_index,
                    column_index: *col_index,
                }
            }
            AppliedOperation::SetColumnWidth {
                sheet_index,
                col_index,
                new_width,
                ..
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    set_layout_value(&mut sheet.column_widths, *col_index, *new_width);
                }
                AppliedOperationResult::SetColumnWidth {
                    sheet_index: *sheet_index,
                    column: ColumnWidthChange {
                        col_index: *col_index,
                        width: *new_width,
                    },
                }
            }
            AppliedOperation::SetRowHeight {
                sheet_index,
                row_index,
                new_height,
                ..
            } => {
                if let Some(sheet) = file_data.sheets.get_mut(*sheet_index) {
                    set_layout_value(&mut sheet.row_heights, *row_index, *new_height);
                }
                AppliedOperationResult::SetRowHeight {
                    sheet_index: *sheet_index,
                    row: RowHeightChange {
                        row_index: *row_index,
                        height: *new_height,
                    },
                }
            }
            AppliedOperation::AddSheet {
                sheet_index,
                sheet_data,
            } => {
                let index = (*sheet_index).min(file_data.sheets.len());
                file_data.sheets.insert(index, sheet_data.clone());
                AppliedOperationResult::AddSheet {
                    sheet_index: index,
                    name: sheet_data.name.clone(),
                    sheet_data: sheet_data.clone(),
                }
            }
            AppliedOperation::DeleteSheet { sheet_index } => {
                let removed_sheet = file_data.sheets.remove(*sheet_index);
                AppliedOperationResult::DeleteSheet {
                    sheet_index: *sheet_index,
                    sheet_data: removed_sheet,
                }
            }
        }
    }

    pub fn execute_cells_and_layout(
        &self,
        file_data: &mut FileData,
    ) -> Option<AppliedOperationResult> {
        match self.operation {
            AppliedOperation::SetCell { .. }
            | AppliedOperation::SetCells { .. }
            | AppliedOperation::SetColumnWidth { .. }
            | AppliedOperation::SetRowHeight { .. } => Some(self.execute(file_data)),
            AppliedOperation::AddRow { .. }
            | AppliedOperation::DeleteRow { .. }
            | AppliedOperation::AddColumn { .. }
            | AppliedOperation::DeleteColumn { .. }
            | AppliedOperation::AddSheet { .. }
            | AppliedOperation::DeleteSheet { .. } => None,
        }
    }
}

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
                sheet_data,
            } => AppliedOperationResult::AddSheet {
                sheet_index: *sheet_index,
                name: sheet_data.name.clone(),
                sheet_data: file_data
                    .sheets
                    .get(*sheet_index)
                    .cloned()
                    .unwrap_or_else(|| sheet_data.clone()),
            },
            AppliedOperation::DeleteSheet { sheet_index } => AppliedOperationResult::DeleteSheet {
                sheet_index: *sheet_index,
                sheet_data: SheetData::default(),
            },
        }
    }
}

impl MutationImpact<'_> {
    pub fn is_noop(&self) -> bool {
        match self.operation {
            AppliedOperation::SetCell {
                old_value,
                new_value,
                ..
            } => old_value == new_value,
            AppliedOperation::SetCells { changes } => changes
                .iter()
                .all(|change| change.old_value == change.new_value),
            AppliedOperation::SetColumnWidth {
                old_width,
                new_width,
                ..
            } => old_width == new_width,
            AppliedOperation::SetRowHeight {
                old_height,
                new_height,
                ..
            } => old_height == new_height,
            _ => false,
        }
    }

    pub fn requires_search_rebuild(&self) -> bool {
        matches!(
            self.operation,
            AppliedOperation::AddRow { .. }
                | AppliedOperation::DeleteRow { .. }
                | AppliedOperation::AddColumn { .. }
                | AppliedOperation::DeleteColumn { .. }
                | AppliedOperation::AddSheet { .. }
                | AppliedOperation::DeleteSheet { .. }
        )
    }

    pub fn is_structure_change(&self) -> bool {
        matches!(
            self.operation,
            AppliedOperation::AddRow { .. }
                | AppliedOperation::DeleteRow { .. }
                | AppliedOperation::AddColumn { .. }
                | AppliedOperation::DeleteColumn { .. }
                | AppliedOperation::AddSheet { .. }
                | AppliedOperation::DeleteSheet { .. }
        )
    }

    pub fn is_row_structure_change(&self) -> bool {
        matches!(
            self.operation,
            AppliedOperation::AddRow { .. } | AppliedOperation::DeleteRow { .. }
        )
    }

    pub fn is_column_structure_change(&self) -> bool {
        matches!(
            self.operation,
            AppliedOperation::AddColumn { .. } | AppliedOperation::DeleteColumn { .. }
        )
    }

    pub fn is_sheet_structure_change(&self) -> bool {
        matches!(
            self.operation,
            AppliedOperation::AddSheet { .. } | AppliedOperation::DeleteSheet { .. }
        )
    }

    pub fn is_layout_change(&self) -> bool {
        matches!(
            self.operation,
            AppliedOperation::SetColumnWidth { .. } | AppliedOperation::SetRowHeight { .. }
        )
    }

    pub fn is_cell_edit(&self) -> bool {
        matches!(
            self.operation,
            AppliedOperation::SetCell { .. } | AppliedOperation::SetCells { .. }
        )
    }
}

fn require_sheet(file_data: &FileData, sheet_index: usize) -> Result<&SheetData, AppError> {
    file_data
        .sheets
        .get(sheet_index)
        .ok_or(AppError::InvalidSheetIndex(sheet_index))
}

fn sheet_row_extent(sheet: &SheetData) -> usize {
    let row_count = sheet.rows.len();
    let merge_extent = sheet
        .merges
        .iter()
        .map(|merge| merge.end_row as usize + 1)
        .max()
        .unwrap_or(0);
    let layout_extent = sheet
        .row_heights
        .as_ref()
        .and_then(|heights| heights.keys().max().map(|index| index + 1))
        .unwrap_or(0);
    row_count.max(merge_extent).max(layout_extent)
}

fn sheet_column_extent(sheet: &SheetData) -> usize {
    let row_extent = sheet.rows.iter().map(Vec::len).max().unwrap_or(0);
    let merge_extent = sheet
        .merges
        .iter()
        .map(|merge| merge.end_col as usize + 1)
        .max()
        .unwrap_or(0);
    let layout_extent = sheet
        .column_widths
        .as_ref()
        .and_then(|widths| widths.keys().max().map(|index| index + 1))
        .unwrap_or(0);
    row_extent.max(merge_extent).max(layout_extent)
}

fn empty_sheet(name: String) -> SheetData {
    SheetData {
        name,
        rows: vec![vec![CellValue::Null; 5]; 5],
        merges: vec![],
        ..Default::default()
    }
}

fn set_layout_value(
    map: &mut Option<std::collections::HashMap<usize, u32>>,
    index: usize,
    value: Option<u32>,
) {
    match value {
        Some(value) => {
            map.get_or_insert_with(Default::default)
                .insert(index, value);
        }
        None => {
            if let Some(values) = map.as_mut() {
                values.remove(&index);
                if values.is_empty() {
                    *map = None;
                }
            }
        }
    }
}

fn shift_layout_map_on_insert(
    map: Option<&mut std::collections::HashMap<usize, u32>>,
    index: usize,
) {
    let Some(map) = map else {
        return;
    };
    if map.is_empty() {
        return;
    }
    let shifted = map
        .drain()
        .map(|(key, value)| {
            let key = if key >= index { key + 1 } else { key };
            (key, value)
        })
        .collect();
    *map = shifted;
}

fn ensure_cell_exists(sheet: &mut SheetData, row: usize, col: usize) {
    let target_width = col + 1;
    while sheet.rows.len() <= row {
        sheet.rows.push(vec![CellValue::Null; target_width]);
    }
    for row_data in &mut sheet.rows {
        if row_data.len() < target_width {
            row_data.resize(target_width, CellValue::Null);
        }
    }
}

fn shift_row_merges_on_insert(merges: &mut Vec<MergeRange>, row_index: usize) {
    let row = row_index as u32;
    for merge in merges {
        if merge.start_row >= row {
            merge.start_row += 1;
            merge.end_row += 1;
        } else if merge.end_row >= row {
            merge.end_row += 1;
        }
    }
}

fn shift_row_merges_on_delete(merges: &mut Vec<MergeRange>, row_index: usize) {
    let row = row_index as u32;
    merges.retain_mut(|merge| {
        if merge.start_row == row && merge.end_row == row {
            return false;
        }
        if merge.start_row > row {
            merge.start_row -= 1;
            merge.end_row -= 1;
        } else if merge.end_row >= row {
            merge.end_row = merge.end_row.saturating_sub(1);
        }
        merge.start_row <= merge.end_row && merge.start_col <= merge.end_col
    });
}

fn shift_column_merges_on_insert(merges: &mut Vec<MergeRange>, col_index: usize) {
    let col = col_index as u16;
    for merge in merges {
        if merge.start_col >= col {
            merge.start_col += 1;
            merge.end_col += 1;
        } else if merge.end_col >= col {
            merge.end_col += 1;
        }
    }
}

fn shift_column_merges_on_delete(merges: &mut Vec<MergeRange>, col_index: usize) {
    let col = col_index as u16;
    merges.retain_mut(|merge| {
        if merge.start_col == col && merge.end_col == col {
            return false;
        }
        if merge.start_col > col {
            merge.start_col -= 1;
            merge.end_col -= 1;
        } else if merge.end_col >= col {
            merge.end_col = merge.end_col.saturating_sub(1);
        }
        merge.start_row <= merge.end_row && merge.start_col <= merge.end_col
    });
}

fn shift_layout_map_on_delete(
    map: Option<&mut std::collections::HashMap<usize, u32>>,
    index: usize,
) {
    let Some(map) = map else {
        return;
    };
    if map.is_empty() {
        return;
    }
    let shifted = map
        .drain()
        .filter_map(|(key, value)| {
            if key == index {
                None
            } else {
                let key = if key > index { key - 1 } else { key };
                Some((key, value))
            }
        })
        .collect();
    *map = shifted;
}
