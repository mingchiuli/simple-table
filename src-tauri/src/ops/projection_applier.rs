use std::collections::HashMap;

use crate::domain::{AppliedOperation, ProjectionMutation, cell_key::parse_cell_key};
use crate::types::{
    AppliedOperationResult, CellChange, CellValue, ColumnChange, ColumnWidthChange,
    DrawingProjection, FileData, MergeRange, ReadOnlyRichProjection, RowChange, RowHeightChange,
    SheetCellChange, SheetData,
};

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
                    while sheet.rows.len() < *row_index {
                        sheet.rows.push(Vec::new());
                    }
                    sheet.rows.insert(*row_index, row_data.clone());
                    shift_layout_map_on_insert(sheet.row_heights.as_mut(), *row_index);
                    shift_row_merges_on_insert(&mut sheet.merges, *row_index);
                    shift_rich_rows_on_insert(&mut sheet.rich, *row_index);
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
                    shift_rich_rows_on_delete(&mut sheet.rich, *row_index);
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
                    if sheet.rows.len() < col_data.len() {
                        sheet.rows.resize_with(col_data.len(), Vec::new);
                    }
                    for (row_index, row) in sheet.rows.iter_mut().enumerate() {
                        let value = col_data.get(row_index).cloned().unwrap_or(CellValue::Null);
                        while row.len() < *col_index {
                            row.push(CellValue::Null);
                        }
                        row.insert(*col_index, value);
                    }
                    shift_layout_map_on_insert(sheet.column_widths.as_mut(), *col_index);
                    shift_column_merges_on_insert(&mut sheet.merges, *col_index);
                    shift_rich_columns_on_insert(&mut sheet.rich, *col_index);
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
                    shift_rich_columns_on_delete(&mut sheet.rich, *col_index);
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
                name,
                row_count,
                column_count,
            } => {
                let index = (*sheet_index).min(file_data.sheets.len());
                let sheet_data = new_sheet_data(name, *row_count, *column_count);
                file_data.sheets.insert(index, sheet_data.clone());
                AppliedOperationResult::AddSheet {
                    sheet_index: index,
                    name: name.clone(),
                    sheet_data,
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

fn new_sheet_data(name: &str, row_count: usize, column_count: usize) -> SheetData {
    SheetData {
        name: name.to_string(),
        rows: vec![vec![CellValue::Null; column_count]; row_count],
        ..Default::default()
    }
}

fn set_layout_value(map: &mut Option<HashMap<usize, u32>>, index: usize, value: Option<u32>) {
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

fn shift_layout_map_on_insert(map: Option<&mut HashMap<usize, u32>>, index: usize) {
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

fn shift_rich_rows_on_insert(rich: &mut ReadOnlyRichProjection, row_index: usize) {
    remap_cell_map(&mut rich.cell_formats, |row, col| {
        Some((if row >= row_index { row + 1 } else { row }, col))
    });
    remap_cell_map(&mut rich.cell_styles, |row, col| {
        Some((if row >= row_index { row + 1 } else { row }, col))
    });
    remap_cell_map(&mut rich.hyperlinks, |row, col| {
        Some((if row >= row_index { row + 1 } else { row }, col))
    });
    remap_index_vec(&mut rich.hidden_rows, |row| {
        Some(if row >= row_index { row + 1 } else { row })
    });
    remap_freeze_pane(&mut rich.freeze_pane, |row, col| {
        Some((if row >= row_index { row + 1 } else { row }, col))
    });
    for drawing in &mut rich.drawings {
        if drawing.from_row as usize >= row_index {
            drawing.from_row += 1;
        }
        if let Some(to_row) = drawing.to_row.as_mut()
            && *to_row as usize >= row_index
        {
            *to_row += 1;
        }
    }
}

fn shift_rich_rows_on_delete(rich: &mut ReadOnlyRichProjection, row_index: usize) {
    remap_cell_map(&mut rich.cell_formats, |row, col| {
        delete_index(row, row_index).map(|row| (row, col))
    });
    remap_cell_map(&mut rich.cell_styles, |row, col| {
        delete_index(row, row_index).map(|row| (row, col))
    });
    remap_cell_map(&mut rich.hyperlinks, |row, col| {
        delete_index(row, row_index).map(|row| (row, col))
    });
    remap_index_vec(&mut rich.hidden_rows, |row| delete_index(row, row_index));
    remap_freeze_pane(&mut rich.freeze_pane, |row, col| {
        delete_index(row, row_index).map(|row| (row, col))
    });
    rich.drawings = rich
        .drawings
        .drain(..)
        .filter_map(|drawing| delete_drawing_row(drawing, row_index))
        .collect();
}

fn shift_rich_columns_on_insert(rich: &mut ReadOnlyRichProjection, col_index: usize) {
    remap_cell_map(&mut rich.cell_formats, |row, col| {
        Some((row, if col >= col_index { col + 1 } else { col }))
    });
    remap_cell_map(&mut rich.cell_styles, |row, col| {
        Some((row, if col >= col_index { col + 1 } else { col }))
    });
    remap_cell_map(&mut rich.hyperlinks, |row, col| {
        Some((row, if col >= col_index { col + 1 } else { col }))
    });
    remap_index_vec(&mut rich.hidden_columns, |col| {
        Some(if col >= col_index { col + 1 } else { col })
    });
    remap_freeze_pane(&mut rich.freeze_pane, |row, col| {
        Some((row, if col >= col_index { col + 1 } else { col }))
    });
    for drawing in &mut rich.drawings {
        if drawing.from_col as usize >= col_index {
            drawing.from_col += 1;
        }
        if let Some(to_col) = drawing.to_col.as_mut()
            && *to_col as usize >= col_index
        {
            *to_col += 1;
        }
    }
}

fn shift_rich_columns_on_delete(rich: &mut ReadOnlyRichProjection, col_index: usize) {
    remap_cell_map(&mut rich.cell_formats, |row, col| {
        delete_index(col, col_index).map(|col| (row, col))
    });
    remap_cell_map(&mut rich.cell_styles, |row, col| {
        delete_index(col, col_index).map(|col| (row, col))
    });
    remap_cell_map(&mut rich.hyperlinks, |row, col| {
        delete_index(col, col_index).map(|col| (row, col))
    });
    remap_index_vec(&mut rich.hidden_columns, |col| delete_index(col, col_index));
    remap_freeze_pane(&mut rich.freeze_pane, |row, col| {
        delete_index(col, col_index).map(|col| (row, col))
    });
    rich.drawings = rich
        .drawings
        .drain(..)
        .filter_map(|drawing| delete_drawing_column(drawing, col_index))
        .collect();
}

fn remap_cell_map<T>(
    values: &mut HashMap<String, T>,
    map: impl Fn(usize, usize) -> Option<(usize, usize)>,
) {
    *values = values
        .drain()
        .filter_map(|(key, value)| {
            let Some((row, col)) = parse_cell_key(&key) else {
                return Some((key, value));
            };
            map(row, col).map(|(row, col)| (excel_cell_key(row, col), value))
        })
        .collect();
}

fn remap_index_vec(values: &mut Vec<usize>, map: impl Fn(usize) -> Option<usize>) {
    let mut shifted: Vec<usize> = values.iter().filter_map(|value| map(*value)).collect();
    shifted.sort_unstable();
    shifted.dedup();
    *values = shifted;
}

fn remap_freeze_pane(
    freeze_pane: &mut Option<crate::types::FreezePaneProjection>,
    map: impl Fn(usize, usize) -> Option<(usize, usize)>,
) {
    let Some(pane) = freeze_pane else {
        return;
    };
    let Some((row, col)) = parse_cell_key(&pane.top_left_cell) else {
        return;
    };
    match map(row, col) {
        Some((row, col)) => pane.top_left_cell = excel_cell_key(row, col),
        None => *freeze_pane = None,
    }
}

fn delete_drawing_row(
    mut drawing: DrawingProjection,
    row_index: usize,
) -> Option<DrawingProjection> {
    let from_row = delete_index(drawing.from_row as usize, row_index).map(|row| row as u32);
    let to_row = drawing
        .to_row
        .and_then(|row| delete_index(row as usize, row_index).map(|row| row as u32));
    if from_row.is_none() && to_row.is_none() {
        return None;
    }
    drawing.from_row = from_row.unwrap_or(row_index as u32);
    drawing.to_row = drawing.to_row.map(|_| to_row.unwrap_or(drawing.from_row));
    Some(drawing)
}

fn delete_drawing_column(
    mut drawing: DrawingProjection,
    col_index: usize,
) -> Option<DrawingProjection> {
    let from_col = delete_index(drawing.from_col as usize, col_index).map(|col| col as u32);
    let to_col = drawing
        .to_col
        .and_then(|col| delete_index(col as usize, col_index).map(|col| col as u32));
    if from_col.is_none() && to_col.is_none() {
        return None;
    }
    drawing.from_col = from_col.unwrap_or(col_index as u32);
    drawing.to_col = drawing.to_col.map(|_| to_col.unwrap_or(drawing.from_col));
    Some(drawing)
}

fn delete_index(index: usize, deleted_index: usize) -> Option<usize> {
    if index < deleted_index {
        Some(index)
    } else if index > deleted_index {
        Some(index - 1)
    } else {
        None
    }
}

fn excel_cell_key(row_index: usize, col_index: usize) -> String {
    let mut col = col_index + 1;
    let mut letters = String::new();
    while col > 0 {
        let rem = (col - 1) % 26;
        letters.insert(0, (b'A' + rem as u8) as char);
        col = (col - 1) / 26;
    }
    format!("{letters}{}", row_index + 1)
}

fn shift_layout_map_on_delete(map: Option<&mut HashMap<usize, u32>>, index: usize) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CellStyleProjection, DrawingKind, HyperlinkProjection};

    fn file_data_with_rich_projection() -> FileData {
        FileData {
            path: String::new(),
            file_name: "projection.xlsx".to_string(),
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows: vec![vec![CellValue::String("A1".to_string())]],
                rich: ReadOnlyRichProjection {
                    cell_styles: [(
                        "B2".to_string(),
                        CellStyleProjection {
                            bold: Some(true),
                            ..Default::default()
                        },
                    )]
                    .into_iter()
                    .collect(),
                    hyperlinks: [(
                        "B3".to_string(),
                        HyperlinkProjection {
                            url: "https://example.com".to_string(),
                            tooltip: None,
                            location: false,
                        },
                    )]
                    .into_iter()
                    .collect(),
                    hidden_rows: vec![2],
                    hidden_columns: vec![1],
                    drawings: vec![DrawingProjection {
                        kind: DrawingKind::Image,
                        from_row: 1,
                        from_col: 1,
                        to_row: Some(2),
                        to_col: Some(2),
                    }],
                    ..Default::default()
                },
                ..Default::default()
            }],
        }
    }

    #[test]
    fn projection_only_structure_edits_shift_rich_metadata() {
        let mut file_data = file_data_with_rich_projection();
        AppliedOperation::AddColumn {
            sheet_index: 0,
            col_index: 1,
            col_data: Vec::new(),
            column_width: None,
        }
        .projection_mutation()
        .execute(&mut file_data);

        let rich = &file_data.sheets[0].rich;
        assert!(rich.cell_styles.contains_key("C2"));
        assert!(rich.hyperlinks.contains_key("C3"));
        assert_eq!(rich.hidden_columns, vec![2]);
        assert_eq!(rich.drawings[0].from_col, 2);
        assert_eq!(rich.drawings[0].to_col, Some(3));

        AppliedOperation::DeleteRow {
            sheet_index: 0,
            row_index: 0,
        }
        .projection_mutation()
        .execute(&mut file_data);

        let rich = &file_data.sheets[0].rich;
        assert!(rich.cell_styles.contains_key("C1"));
        assert!(rich.hyperlinks.contains_key("C2"));
        assert_eq!(rich.hidden_rows, vec![1]);
        assert_eq!(rich.drawings[0].from_row, 0);
        assert_eq!(rich.drawings[0].to_row, Some(1));
    }

    #[test]
    fn add_column_projection_preserves_sparse_column_position() {
        let mut file_data = FileData {
            path: String::new(),
            file_name: "sparse.xlsx".to_string(),
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows: vec![vec![CellValue::String("A1".to_string())], vec![]],
                ..Default::default()
            }],
        };

        AppliedOperation::AddColumn {
            sheet_index: 0,
            col_index: 3,
            col_data: vec![
                CellValue::String("D1".to_string()),
                CellValue::String("D2".to_string()),
            ],
            column_width: None,
        }
        .projection_mutation()
        .execute(&mut file_data);

        assert_eq!(
            file_data.sheets[0].rows,
            vec![
                vec![
                    CellValue::String("A1".to_string()),
                    CellValue::Null,
                    CellValue::Null,
                    CellValue::String("D1".to_string()),
                ],
                vec![
                    CellValue::Null,
                    CellValue::Null,
                    CellValue::Null,
                    CellValue::String("D2".to_string()),
                ],
            ]
        );
    }
}
