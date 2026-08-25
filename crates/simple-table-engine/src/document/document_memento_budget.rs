use crate::document_data::{
    DocumentData, DocumentSheet, Drawing, FreezePane, MergeRange, RichMetadata,
};
use std::collections::HashSet;

use crate::document::backing::document_body::SpreadsheetDocumentBody;
use crate::document::backing::rich_projection::{
    drawing_column_scope_affected, drawing_row_scope_affected,
};
use crate::document::document_memento::{
    ColumnStructureMemento, FileStructureMemento, LayoutMemento, RichProjectionMemento,
    RowStructureMemento, SheetShapeMemento, SheetTailMemento,
};
use crate::document::formula_coordinator::FormulaCoordinator;
use crate::document_resource_estimator::{
    estimate_cell_format_bytes, estimate_cell_style_bytes, estimate_cell_value_bytes,
    estimate_freeze_pane_bytes, estimate_hyperlink_bytes, estimate_sheet_data_bytes,
};
use crate::domain::{AppliedOperation, CellValue, cell_key::parse_cell_key};
use crate::formula::cell_ref::FormulaCellRef;

pub(crate) fn estimate_memento_side_bytes(
    projection: &DocumentData,
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
        AppliedOperation::InsertImage { bytes, .. } => bytes.len().saturating_add(512),
        AppliedOperation::UpdateImage { .. } => 512,
        AppliedOperation::DeleteImage {
            sheet_index, image, ..
        } => body
            .image_bytes(*sheet_index, image.z_index)
            .map(|bytes| bytes.len().saturating_add(512))
            .unwrap_or(512),
        AppliedOperation::SortRows(sort) => {
            let permutation_bytes = sort
                .permutation
                .len()
                .saturating_mul(std::mem::size_of::<usize>())
                .saturating_mul(2);
            let formula_bytes = sort
                .before_formulas
                .iter()
                .chain(&sort.after_formulas)
                .map(|formula| formula.formula.len().saturating_add(24))
                .sum::<usize>();
            permutation_bytes
                .saturating_add(formula_bytes)
                .saturating_add(256)
        }
    }
}

fn estimate_cell_memento_bytes(
    projection: &DocumentData,
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
    let touched_sheets = positions
        .iter()
        .map(|(sheet_index, _, _)| *sheet_index)
        .collect::<HashSet<_>>();
    let cell_bytes = positions
        .into_iter()
        .map(|(sheet_index, row, col)| {
            estimate_cell_value_bytes(&projection_cell(projection, sheet_index, row, col))
        })
        .sum::<usize>();
    let sheet_shape_bytes = touched_sheets
        .into_iter()
        .map(|sheet_index| {
            projection
                .sheets
                .get(sheet_index)
                .map(estimate_sheet_shape_memento_bytes)
                .unwrap_or(std::mem::size_of::<SheetShapeMemento>())
        })
        .sum::<usize>();
    cell_bytes + sheet_shape_bytes + usize::from(formula_capabilities_may_change)
}

fn estimate_file_structure_memento_bytes(
    file_data: &DocumentData,
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
        AppliedOperation::SortRows(_) => std::mem::size_of::<FileStructureMemento>(),
        AppliedOperation::InsertImage { .. }
        | AppliedOperation::UpdateImage { .. }
        | AppliedOperation::DeleteImage { .. } => std::mem::size_of::<FileStructureMemento>(),
    }
}

fn projection_cell(
    file_data: &DocumentData,
    sheet_index: usize,
    row: usize,
    col: usize,
) -> CellValue {
    file_data
        .sheets
        .get(sheet_index)
        .and_then(|sheet| sheet.rows.get(row))
        .and_then(|row_data| row_data.get(col))
        .cloned()
        .unwrap_or(CellValue::Null)
}

fn estimate_sheet_shape_memento_bytes(sheet: &DocumentSheet) -> usize {
    std::mem::size_of::<SheetShapeMemento>()
        + sheet.rows.len() * std::mem::size_of::<usize>()
        + estimate_protected_rich_cell_count(sheet) * std::mem::size_of::<(usize, usize)>()
}

fn estimate_protected_rich_cell_count(sheet: &DocumentSheet) -> usize {
    sheet.rich.cell_formats.len()
        + sheet.rich.cell_styles.len()
        + sheet.rich.hyperlinks.len()
        + sheet.rich.drawings.len() * 2
        + sheet.rich.images.len() * 2
}

fn estimate_rich_projection_tail_bytes(
    rich: &RichMetadata,
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
            .iter()
            .filter(|(key, _)| matches_cell(key))
            .map(|(cell, format)| cell.len() + estimate_cell_format_bytes(format))
            .sum::<usize>()
        + rich
            .cell_styles
            .iter()
            .filter(|(key, _)| matches_cell(key))
            .map(|(cell, style)| cell.len() + estimate_cell_style_bytes(style))
            .sum::<usize>()
        + rich
            .hidden_rows
            .iter()
            .filter(|row| min_row.is_some_and(|min_row| **row >= min_row))
            .count()
            * std::mem::size_of::<usize>()
        + rich
            .hidden_columns
            .iter()
            .filter(|col| min_col.is_some_and(|min_col| **col >= min_col))
            .count()
            * std::mem::size_of::<usize>()
        + rich
            .freeze_pane
            .as_ref()
            .filter(|pane| freeze_pane_matches_scope(pane, min_row, min_col))
            .map(estimate_freeze_pane_bytes)
            .unwrap_or_default()
        + rich
            .hyperlinks
            .iter()
            .filter(|(key, _)| matches_cell(key))
            .map(|(cell, hyperlink)| cell.len() + estimate_hyperlink_bytes(hyperlink))
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
            * std::mem::size_of::<Drawing>()
}

fn freeze_pane_matches_scope(
    freeze_pane: &FreezePane,
    min_row: Option<usize>,
    min_col: Option<usize>,
) -> bool {
    parse_cell_key(&freeze_pane.top_left_cell).is_some_and(|(row, col)| {
        min_row.is_some_and(|min_row| row >= min_row)
            || min_col.is_some_and(|min_col| col >= min_col)
    })
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
        | AppliedOperation::DeleteSheet { .. }
        | AppliedOperation::InsertImage { .. }
        | AppliedOperation::UpdateImage { .. }
        | AppliedOperation::DeleteImage { .. }
        | AppliedOperation::SortRows(_) => false,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_data::{CellStyle, DrawingKind, Hyperlink};

    fn freeze(top_left_cell: &str) -> FreezePane {
        FreezePane {
            top_left_cell: top_left_cell.to_string(),
            horizontal_split: 1.0,
            vertical_split: 1.0,
            active_pane: "bottomRight".to_string(),
            state: "frozen".to_string(),
        }
    }

    #[test]
    fn rich_tail_budget_includes_hidden_rows_columns_and_freeze_pane() {
        let row_freeze = freeze("A5");
        let row_tail = estimate_rich_projection_tail_bytes(
            &RichMetadata {
                hidden_rows: vec![0, 3, 4],
                freeze_pane: Some(row_freeze.clone()),
                ..Default::default()
            },
            Some(3),
            None,
        );
        let empty_row_tail =
            estimate_rich_projection_tail_bytes(&RichMetadata::default(), Some(3), None);

        assert!(
            row_tail
                >= empty_row_tail
                    + 2 * std::mem::size_of::<usize>()
                    + estimate_freeze_pane_bytes(&row_freeze)
        );

        let column_freeze = freeze("E1");
        let column_tail = estimate_rich_projection_tail_bytes(
            &RichMetadata {
                hidden_columns: vec![0, 4, 5],
                freeze_pane: Some(column_freeze.clone()),
                ..Default::default()
            },
            None,
            Some(4),
        );
        let empty_column_tail =
            estimate_rich_projection_tail_bytes(&RichMetadata::default(), None, Some(4));

        assert!(
            column_tail
                >= empty_column_tail
                    + 2 * std::mem::size_of::<usize>()
                    + estimate_freeze_pane_bytes(&column_freeze)
        );
    }

    #[test]
    fn rich_tail_budget_includes_hyperlink_value_size() {
        let short = estimate_rich_projection_tail_bytes(
            &RichMetadata {
                hyperlinks: [(
                    "A2".to_string(),
                    Hyperlink {
                        url: "https://x.test".to_string(),
                        tooltip: None,
                        location: false,
                    },
                )]
                .into_iter()
                .collect(),
                ..Default::default()
            },
            Some(0),
            None,
        );
        let long = estimate_rich_projection_tail_bytes(
            &RichMetadata {
                hyperlinks: [(
                    "A2".to_string(),
                    Hyperlink {
                        url: format!("https://x.test/{}", "a".repeat(1024)),
                        tooltip: Some("tooltip".repeat(64)),
                        location: false,
                    },
                )]
                .into_iter()
                .collect(),
                ..Default::default()
            },
            Some(0),
            None,
        );

        assert!(long > short + 1024);
    }

    #[test]
    fn cell_memento_budget_includes_sheet_shape_rows_and_rich_positions() {
        let mut file_data = DocumentData {
            path: String::new(),
            file_name: "shape-budget.xlsx".to_string(),
            sheets: vec![DocumentSheet {
                name: "Sheet1".to_string(),
                rows: vec![vec![CellValue::Null]; 12],
                rich: RichMetadata {
                    cell_styles: [(
                        "A1".to_string(),
                        CellStyle {
                            bold: Some(true),
                            ..Default::default()
                        },
                    )]
                    .into_iter()
                    .collect(),
                    hyperlinks: [(
                        "B2".to_string(),
                        Hyperlink {
                            url: "https://example.com".to_string(),
                            tooltip: None,
                            location: false,
                        },
                    )]
                    .into_iter()
                    .collect(),
                    drawings: vec![Drawing {
                        kind: DrawingKind::Chart,
                        from_row: 1,
                        from_col: 1,
                        to_row: Some(3),
                        to_col: Some(3),
                    }],
                    ..Default::default()
                },
                ..Default::default()
            }],
        };
        let formulas = FormulaCoordinator::new(&mut file_data);

        let estimate = estimate_cell_memento_bytes(
            &file_data,
            &formulas,
            [FormulaCellRef {
                sheet_index: 0,
                row: 0,
                col: 0,
            }],
            false,
        );

        assert!(
            estimate
                >= 12 * std::mem::size_of::<usize>() + 4 * std::mem::size_of::<(usize, usize)>()
        );
    }
}
