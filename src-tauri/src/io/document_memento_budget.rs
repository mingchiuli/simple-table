use std::collections::{HashMap, HashSet};

use crate::formula::cell_ref::FormulaCellRef;
use crate::io::document_body::SpreadsheetDocumentBody;
use crate::io::document_memento::{
    ColumnStructureMemento, FileStructureMemento, LayoutMemento, RichProjectionMemento,
    RowStructureMemento, SheetShapeMemento, SheetTailMemento,
};
use crate::io::formula_coordinator::FormulaCoordinator;
use crate::io::rich_projection::{
    drawing_column_scope_affected, drawing_row_scope_affected, parse_cell_key,
};
use crate::ops::AppliedOperation;
use crate::types::{CellValue, FileData, MergeRange, ReadOnlyRichProjection, SheetData};

pub(crate) fn estimate_memento_side_bytes(
    projection: &FileData,
    body: &SpreadsheetDocumentBody,
    formulas: &mut FormulaCoordinator,
    operation: &AppliedOperation,
) -> usize {
    match operation {
        AppliedOperation::SetCell {
            sheet_index,
            row,
            col,
            ..
        } => estimate_cell_memento_bytes(
            projection,
            formulas,
            [FormulaCellRef {
                sheet_index: *sheet_index,
                row: *row,
                col: *col,
            }],
            operation_may_change_formula_capabilities(operation),
        ),
        AppliedOperation::SetCells { changes } => estimate_cell_memento_bytes(
            projection,
            formulas,
            changes.iter().map(|change| FormulaCellRef {
                sheet_index: change.sheet_index,
                row: change.row,
                col: change.col,
            }),
            operation_may_change_formula_capabilities(operation),
        ),
        AppliedOperation::SetColumnWidth { .. } | AppliedOperation::SetRowHeight { .. } => {
            std::mem::size_of::<LayoutMemento>() + 64
        }
        AppliedOperation::AddRow { .. }
        | AppliedOperation::DeleteRow { .. }
        | AppliedOperation::AddColumn { .. }
        | AppliedOperation::DeleteColumn { .. }
        | AppliedOperation::AddSheet { .. }
        | AppliedOperation::DeleteSheet { .. } => {
            let formula_sheet_indexes =
                formulas.structure_memento_sheet_indexes(projection, operation);
            estimate_file_structure_memento_bytes(projection, operation)
                + body.estimate_structure_memento_bytes(operation, formula_sheet_indexes)
        }
    }
}

fn estimate_cell_memento_bytes(
    projection: &FileData,
    formulas: &FormulaCoordinator,
    changed_cells: impl IntoIterator<Item = FormulaCellRef>,
    formula_capabilities_may_change: bool,
) -> usize {
    let changed_cells: Vec<FormulaCellRef> = changed_cells.into_iter().collect();
    let formula_cells =
        formulas.impacted_cells_for_memento(changed_cells.iter().copied(), projection);
    let mut positions = Vec::new();
    let mut seen = HashSet::new();
    for cell in changed_cells.into_iter().chain(formula_cells) {
        push_unique_position(
            &mut positions,
            &mut seen,
            cell.sheet_index,
            cell.row,
            cell.col,
        );
    }
    let sheet_shape_count = positions
        .iter()
        .map(|(sheet_index, _, _)| *sheet_index)
        .collect::<HashSet<_>>()
        .len();
    let cell_bytes = positions
        .into_iter()
        .map(|(sheet_index, row, col)| {
            estimate_cell_value_bytes(&projection_cell(projection, sheet_index, row, col))
        })
        .sum::<usize>();
    cell_bytes
        + sheet_shape_count * std::mem::size_of::<SheetShapeMemento>()
        + usize::from(formula_capabilities_may_change)
}

fn estimate_file_structure_memento_bytes(
    file_data: &FileData,
    operation: &AppliedOperation,
) -> usize {
    let sheet_count = file_data.sheets.len();
    match operation {
        AppliedOperation::AddRow {
            sheet_index,
            row_index,
            ..
        }
        | AppliedOperation::DeleteRow {
            sheet_index,
            row_index,
        } => file_data
            .sheets
            .get(*sheet_index)
            .map(|sheet| {
                std::mem::size_of::<RowStructureMemento>()
                    + sheet
                        .rows
                        .get(*row_index)
                        .map(|row| row.iter().map(estimate_cell_value_bytes).sum::<usize>())
                        .unwrap_or_default()
                    + sheet.merges.len() * std::mem::size_of::<MergeRange>()
                    + sheet
                        .row_heights
                        .as_ref()
                        .map(|heights| heights.len() * 24)
                        .unwrap_or_default()
                    + estimate_rich_projection_tail_bytes(&sheet.rich, Some(*row_index), None)
            })
            .unwrap_or(std::mem::size_of::<FileStructureMemento>() + sheet_count),
        AppliedOperation::AddColumn {
            sheet_index,
            col_index,
            ..
        }
        | AppliedOperation::DeleteColumn {
            sheet_index,
            col_index,
        } => file_data
            .sheets
            .get(*sheet_index)
            .map(|sheet| {
                std::mem::size_of::<ColumnStructureMemento>()
                    + sheet.rows.len() * std::mem::size_of::<usize>()
                    + sheet
                        .rows
                        .iter()
                        .filter_map(|row| row.get(*col_index))
                        .map(estimate_cell_value_bytes)
                        .sum::<usize>()
                    + sheet.merges.len() * std::mem::size_of::<MergeRange>()
                    + sheet
                        .column_widths
                        .as_ref()
                        .map(|widths| widths.len() * 24)
                        .unwrap_or_default()
                    + estimate_rich_projection_tail_bytes(&sheet.rich, None, Some(*col_index))
            })
            .unwrap_or(std::mem::size_of::<FileStructureMemento>() + sheet_count),
        AppliedOperation::AddSheet { sheet_index, .. }
        | AppliedOperation::DeleteSheet { sheet_index } => {
            let start = (*sheet_index).min(sheet_count);
            std::mem::size_of::<SheetTailMemento>()
                + file_data.sheets[start..]
                    .iter()
                    .map(estimate_sheet_data_bytes)
                    .sum::<usize>()
        }
        AppliedOperation::SetCell { .. }
        | AppliedOperation::SetCells { .. }
        | AppliedOperation::SetColumnWidth { .. }
        | AppliedOperation::SetRowHeight { .. } => std::mem::size_of::<FileStructureMemento>(),
    }
}

fn projection_cell(file_data: &FileData, sheet_index: usize, row: usize, col: usize) -> CellValue {
    file_data
        .sheets
        .get(sheet_index)
        .and_then(|sheet| sheet.rows.get(row))
        .and_then(|row_data| row_data.get(col))
        .cloned()
        .unwrap_or(CellValue::Null)
}

fn estimate_sheet_data_bytes(sheet: &SheetData) -> usize {
    std::mem::size_of::<SheetData>()
        + sheet.name.len()
        + sheet
            .rows
            .iter()
            .map(|row| row.iter().map(estimate_cell_value_bytes).sum::<usize>())
            .sum::<usize>()
        + sheet.merges.len() * std::mem::size_of::<MergeRange>()
        + sheet
            .column_widths
            .as_ref()
            .map(|widths| widths.len() * 24)
            .unwrap_or_default()
        + sheet
            .row_heights
            .as_ref()
            .map(|heights| heights.len() * 24)
            .unwrap_or_default()
        + estimate_rich_projection_bytes(&sheet.rich)
}

fn estimate_cell_value_bytes(cell: &CellValue) -> usize {
    match cell {
        CellValue::Null => std::mem::size_of::<CellValue>(),
        CellValue::String(value) => std::mem::size_of::<CellValue>() + value.len(),
        CellValue::Number(value) => std::mem::size_of::<CellValue>() + value.to_string().len(),
        CellValue::Boolean(_) => std::mem::size_of::<CellValue>(),
        CellValue::Formula {
            formula,
            cached_value,
            error,
        } => {
            std::mem::size_of::<CellValue>()
                + formula.len()
                + estimate_cell_value_bytes(cached_value)
                + error.as_ref().map(String::len).unwrap_or_default()
        }
    }
}

fn estimate_rich_projection_bytes(rich: &ReadOnlyRichProjection) -> usize {
    std::mem::size_of::<ReadOnlyRichProjection>()
        + estimate_cell_key_map_bytes(&rich.cell_formats)
        + estimate_cell_key_map_bytes(&rich.cell_styles)
        + rich.hidden_rows.len() * std::mem::size_of::<usize>()
        + rich.hidden_columns.len() * std::mem::size_of::<usize>()
        + estimate_cell_key_map_bytes(&rich.hyperlinks)
        + rich.drawings.len() * std::mem::size_of::<crate::types::DrawingProjection>()
}

fn estimate_rich_projection_tail_bytes(
    rich: &ReadOnlyRichProjection,
    min_row: Option<usize>,
    min_col: Option<usize>,
) -> usize {
    let matches_cell = |key: &str| {
        parse_cell_key(key)
            .map(|(row, col)| {
                min_row.is_some_and(|min_row| row >= min_row)
                    || min_col.is_some_and(|min_col| col >= min_col)
            })
            .unwrap_or(false)
    };
    std::mem::size_of::<RichProjectionMemento>()
        + rich
            .cell_formats
            .keys()
            .filter(|key| matches_cell(key))
            .map(String::len)
            .sum::<usize>()
        + rich
            .cell_styles
            .keys()
            .filter(|key| matches_cell(key))
            .map(String::len)
            .sum::<usize>()
        + rich
            .hyperlinks
            .keys()
            .filter(|key| matches_cell(key))
            .map(String::len)
            .sum::<usize>()
        + rich
            .drawings
            .iter()
            .filter(|drawing| {
                min_row.is_some_and(|min_row| drawing_row_scope_affected(drawing, min_row))
                    || min_col
                        .is_some_and(|min_col| drawing_column_scope_affected(drawing, min_col))
            })
            .count()
            * std::mem::size_of::<crate::types::DrawingProjection>()
}

fn estimate_cell_key_map_bytes<T>(values: &HashMap<String, T>) -> usize {
    values
        .keys()
        .map(|key| key.len() + std::mem::size_of::<T>())
        .sum()
}

fn operation_may_change_formula_capabilities(operation: &AppliedOperation) -> bool {
    match operation {
        AppliedOperation::SetCell {
            old_value,
            new_value,
            ..
        } => formula_capability_signature(old_value) != formula_capability_signature(new_value),
        AppliedOperation::SetCells { changes } => changes.iter().any(|change| {
            formula_capability_signature(&change.old_value)
                != formula_capability_signature(&change.new_value)
        }),
        AppliedOperation::AddRow { .. }
        | AppliedOperation::DeleteRow { .. }
        | AppliedOperation::AddColumn { .. }
        | AppliedOperation::DeleteColumn { .. }
        | AppliedOperation::SetColumnWidth { .. }
        | AppliedOperation::SetRowHeight { .. }
        | AppliedOperation::AddSheet { .. }
        | AppliedOperation::DeleteSheet { .. } => false,
    }
}

fn formula_capability_signature(value: &CellValue) -> Option<&str> {
    match value {
        CellValue::Formula { formula, .. } => Some(formula.as_str()),
        _ => None,
    }
}

fn push_unique_position(
    positions: &mut Vec<(usize, usize, usize)>,
    seen: &mut HashSet<(usize, usize, usize)>,
    sheet_index: usize,
    row: usize,
    col: usize,
) {
    if seen.insert((sheet_index, row, col)) {
        positions.push((sheet_index, row, col));
    }
}
