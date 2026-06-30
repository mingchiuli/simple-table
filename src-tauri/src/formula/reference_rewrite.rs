use formualizer_parse::parser::{ASTNode, ASTNodeType, ReferenceType};
use formualizer_parse::{canonical_formula, parse};

#[derive(Clone, Copy)]
pub enum StructureShift {
    InsertRows { row_index: usize, count: usize },
    DeleteRows { row_index: usize, count: usize },
    InsertColumns { col_index: usize, count: usize },
    DeleteColumns { col_index: usize, count: usize },
}

pub fn adjust_formula_references(
    formula: &str,
    target_sheet_name: &str,
    current_sheet_name: &str,
    shift: StructureShift,
) -> String {
    let has_equals = formula.starts_with('=');
    let formula_to_parse = if has_equals {
        formula.to_string()
    } else {
        format!("={formula}")
    };
    let mut ast = match parse(&formula_to_parse) {
        Ok(ast) => ast,
        Err(_) => return formula.to_string(),
    };

    if !rewrite_ast_references(&mut ast, target_sheet_name, current_sheet_name, shift) {
        return formula.to_string();
    }

    let adjusted = canonical_formula(&ast);
    if has_equals {
        adjusted
    } else {
        adjusted
            .strip_prefix('=')
            .unwrap_or(adjusted.as_str())
            .to_string()
    }
}

pub fn invalidate_deleted_sheet_references(
    formula: &str,
    deleted_sheet_name: &str,
    current_sheet_name: &str,
) -> String {
    let has_equals = formula.starts_with('=');
    let formula_to_parse = if has_equals {
        formula.to_string()
    } else {
        format!("={formula}")
    };
    let mut ast = match parse(&formula_to_parse) {
        Ok(ast) => ast,
        Err(_) => return formula.to_string(),
    };

    if !invalidate_ast_sheet_references(&mut ast, deleted_sheet_name, current_sheet_name) {
        return formula.to_string();
    }

    let adjusted = canonical_formula(&ast);
    if has_equals {
        adjusted
    } else {
        adjusted
            .strip_prefix('=')
            .unwrap_or(adjusted.as_str())
            .to_string()
    }
}

fn rewrite_ast_references(
    ast: &mut ASTNode,
    target_sheet_name: &str,
    current_sheet_name: &str,
    shift: StructureShift,
) -> bool {
    match &mut ast.node_type {
        ASTNodeType::Reference {
            original,
            reference,
        } => {
            if !reference_targets_sheet(reference, target_sheet_name, current_sheet_name) {
                return false;
            }
            let Some(next_reference) = adjust_reference(reference.clone(), shift) else {
                *original = "#REF!".to_string();
                *reference = ReferenceType::NamedRange("#REF!".to_string());
                return true;
            };
            *original = next_reference.normalise();
            *reference = next_reference;
            true
        }
        ASTNodeType::UnaryOp { expr, .. } => {
            rewrite_ast_references(expr, target_sheet_name, current_sheet_name, shift)
        }
        ASTNodeType::BinaryOp { left, right, .. } => {
            let left_changed =
                rewrite_ast_references(left, target_sheet_name, current_sheet_name, shift);
            let right_changed =
                rewrite_ast_references(right, target_sheet_name, current_sheet_name, shift);
            left_changed || right_changed
        }
        ASTNodeType::Function { args, .. } => {
            let mut changed = false;
            for arg in args {
                changed |=
                    rewrite_ast_references(arg, target_sheet_name, current_sheet_name, shift);
            }
            changed
        }
        ASTNodeType::Call { callee, args } => {
            let mut changed =
                rewrite_ast_references(callee, target_sheet_name, current_sheet_name, shift);
            for arg in args {
                changed |=
                    rewrite_ast_references(arg, target_sheet_name, current_sheet_name, shift);
            }
            changed
        }
        ASTNodeType::Array(rows) => {
            let mut changed = false;
            for row in rows {
                for item in row {
                    changed |=
                        rewrite_ast_references(item, target_sheet_name, current_sheet_name, shift);
                }
            }
            changed
        }
        ASTNodeType::Literal(_) => false,
    }
}

fn invalidate_ast_sheet_references(
    ast: &mut ASTNode,
    target_sheet_name: &str,
    current_sheet_name: &str,
) -> bool {
    match &mut ast.node_type {
        ASTNodeType::Reference {
            original,
            reference,
        } => {
            if !reference_targets_sheet(reference, target_sheet_name, current_sheet_name) {
                return false;
            }
            *original = "#REF!".to_string();
            *reference = ReferenceType::NamedRange("#REF!".to_string());
            true
        }
        ASTNodeType::UnaryOp { expr, .. } => {
            invalidate_ast_sheet_references(expr, target_sheet_name, current_sheet_name)
        }
        ASTNodeType::BinaryOp { left, right, .. } => {
            let left_changed =
                invalidate_ast_sheet_references(left, target_sheet_name, current_sheet_name);
            let right_changed =
                invalidate_ast_sheet_references(right, target_sheet_name, current_sheet_name);
            left_changed || right_changed
        }
        ASTNodeType::Function { args, .. } => args.iter_mut().fold(false, |changed, arg| {
            invalidate_ast_sheet_references(arg, target_sheet_name, current_sheet_name) || changed
        }),
        ASTNodeType::Call { callee, args } => {
            let callee_changed =
                invalidate_ast_sheet_references(callee, target_sheet_name, current_sheet_name);
            args.iter_mut().fold(callee_changed, |changed, arg| {
                invalidate_ast_sheet_references(arg, target_sheet_name, current_sheet_name)
                    || changed
            })
        }
        ASTNodeType::Array(rows) => {
            let mut changed = false;
            for row in rows {
                for item in row {
                    changed |= invalidate_ast_sheet_references(
                        item,
                        target_sheet_name,
                        current_sheet_name,
                    );
                }
            }
            changed
        }
        ASTNodeType::Literal(_) => false,
    }
}

fn reference_targets_sheet(
    reference: &ReferenceType,
    target_sheet_name: &str,
    current_sheet_name: &str,
) -> bool {
    match reference {
        ReferenceType::Cell { sheet, .. } | ReferenceType::Range { sheet, .. } => sheet
            .as_deref()
            .map(|name| name == target_sheet_name)
            .unwrap_or(current_sheet_name == target_sheet_name),
        ReferenceType::Cell3D {
            sheet_first,
            sheet_last,
            ..
        }
        | ReferenceType::Range3D {
            sheet_first,
            sheet_last,
            ..
        } => sheet_first == target_sheet_name || sheet_last == target_sheet_name,
        ReferenceType::External(_) | ReferenceType::Table(_) | ReferenceType::NamedRange(_) => {
            false
        }
    }
}

fn adjust_reference(reference: ReferenceType, shift: StructureShift) -> Option<ReferenceType> {
    match reference {
        ReferenceType::Cell {
            sheet,
            row,
            col,
            row_abs,
            col_abs,
        } => {
            let (row, col) = adjust_cell_axis(row, col, shift)?;
            Some(ReferenceType::Cell {
                sheet,
                row,
                col,
                row_abs,
                col_abs,
            })
        }
        ReferenceType::Range {
            sheet,
            start_row,
            start_col,
            end_row,
            end_col,
            start_row_abs,
            start_col_abs,
            end_row_abs,
            end_col_abs,
        } => {
            let (start_row, end_row) =
                adjust_optional_range_axis(start_row, end_row, shift, Axis::Row)?;
            let (start_col, end_col) =
                adjust_optional_range_axis(start_col, end_col, shift, Axis::Column)?;
            Some(ReferenceType::Range {
                sheet,
                start_row,
                start_col,
                end_row,
                end_col,
                start_row_abs,
                start_col_abs,
                end_row_abs,
                end_col_abs,
            })
        }
        ReferenceType::Cell3D {
            sheet_first,
            sheet_last,
            row,
            col,
            row_abs,
            col_abs,
        } => {
            let (row, col) = adjust_cell_axis(row, col, shift)?;
            Some(ReferenceType::Cell3D {
                sheet_first,
                sheet_last,
                row,
                col,
                row_abs,
                col_abs,
            })
        }
        ReferenceType::Range3D {
            sheet_first,
            sheet_last,
            start_row,
            start_col,
            end_row,
            end_col,
            start_row_abs,
            start_col_abs,
            end_row_abs,
            end_col_abs,
        } => {
            let (start_row, end_row) =
                adjust_optional_range_axis(start_row, end_row, shift, Axis::Row)?;
            let (start_col, end_col) =
                adjust_optional_range_axis(start_col, end_col, shift, Axis::Column)?;
            Some(ReferenceType::Range3D {
                sheet_first,
                sheet_last,
                start_row,
                start_col,
                end_row,
                end_col,
                start_row_abs,
                start_col_abs,
                end_row_abs,
                end_col_abs,
            })
        }
        ReferenceType::External(_) | ReferenceType::Table(_) | ReferenceType::NamedRange(_) => {
            Some(reference)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    Row,
    Column,
}

fn adjust_cell_axis(row: u32, col: u32, shift: StructureShift) -> Option<(u32, u32)> {
    let mut row_index = to_zero_based(row)?;
    let mut col_index = to_zero_based(col)?;
    match shift {
        StructureShift::InsertRows {
            row_index: at,
            count,
        } => {
            if row_index >= at {
                row_index = row_index.checked_add(count)?;
            }
        }
        StructureShift::DeleteRows {
            row_index: at,
            count,
        } => {
            row_index = adjust_deleted_cell_axis(row_index, at, count)?;
        }
        StructureShift::InsertColumns {
            col_index: at,
            count,
        } => {
            if col_index >= at {
                col_index = col_index.checked_add(count)?;
            }
        }
        StructureShift::DeleteColumns {
            col_index: at,
            count,
        } => {
            col_index = adjust_deleted_cell_axis(col_index, at, count)?;
        }
    }
    Some((to_one_based(row_index)?, to_one_based(col_index)?))
}

fn adjust_optional_range_axis(
    start: Option<u32>,
    end: Option<u32>,
    shift: StructureShift,
    axis: Axis,
) -> Option<(Option<u32>, Option<u32>)> {
    if !shift_applies_to_axis(shift, axis) {
        return Some((start, end));
    }

    match shift {
        StructureShift::InsertRows { row_index, count } if axis == Axis::Row => {
            adjust_optional_range_for_insert(start, end, row_index, count)
        }
        StructureShift::DeleteRows { row_index, count } if axis == Axis::Row => {
            adjust_optional_range_for_delete(start, end, row_index, count)
        }
        StructureShift::InsertColumns { col_index, count } if axis == Axis::Column => {
            adjust_optional_range_for_insert(start, end, col_index, count)
        }
        StructureShift::DeleteColumns { col_index, count } if axis == Axis::Column => {
            adjust_optional_range_for_delete(start, end, col_index, count)
        }
        _ => Some((start, end)),
    }
}

fn shift_applies_to_axis(shift: StructureShift, axis: Axis) -> bool {
    matches!(
        (shift, axis),
        (StructureShift::InsertRows { .. }, Axis::Row)
            | (StructureShift::DeleteRows { .. }, Axis::Row)
            | (StructureShift::InsertColumns { .. }, Axis::Column)
            | (StructureShift::DeleteColumns { .. }, Axis::Column)
    )
}

fn adjust_optional_range_for_insert(
    start: Option<u32>,
    end: Option<u32>,
    insert_index: usize,
    count: usize,
) -> Option<(Option<u32>, Option<u32>)> {
    Some((
        adjust_optional_axis_for_insert(start, insert_index, count)?,
        adjust_optional_axis_for_insert(end, insert_index, count)?,
    ))
}

fn adjust_optional_range_for_delete(
    start: Option<u32>,
    end: Option<u32>,
    delete_start: usize,
    count: usize,
) -> Option<(Option<u32>, Option<u32>)> {
    match (start, end) {
        (Some(start), Some(end)) => {
            let (start, end) = adjust_deleted_range_axis(
                to_zero_based(start)?,
                to_zero_based(end)?,
                delete_start,
                count,
            )?;
            Some((Some(to_one_based(start)?), Some(to_one_based(end)?)))
        }
        (Some(start), None) => {
            let start = adjust_deleted_cell_axis(to_zero_based(start)?, delete_start, count)?;
            Some((Some(to_one_based(start)?), None))
        }
        (None, Some(end)) => {
            let end = adjust_deleted_cell_axis(to_zero_based(end)?, delete_start, count)?;
            Some((None, Some(to_one_based(end)?)))
        }
        (None, None) => Some((None, None)),
    }
}

fn adjust_optional_axis_for_insert(
    value: Option<u32>,
    insert_index: usize,
    count: usize,
) -> Option<Option<u32>> {
    let Some(value) = value else {
        return Some(None);
    };
    let index = to_zero_based(value)?;
    if index >= insert_index {
        return Some(Some(to_one_based(index.checked_add(count)?)?));
    }
    Some(Some(value))
}

fn adjust_deleted_cell_axis(index: usize, delete_start: usize, count: usize) -> Option<usize> {
    let delete_end = delete_start.checked_add(count)?;
    if (delete_start..delete_end).contains(&index) {
        return None;
    }
    if index >= delete_end {
        return Some(index.saturating_sub(count));
    }
    Some(index)
}

fn adjust_deleted_range_axis(
    start: usize,
    end: usize,
    delete_start: usize,
    count: usize,
) -> Option<(usize, usize)> {
    let delete_end = delete_start.checked_add(count)?.checked_sub(1)?;
    if start > end {
        return adjust_deleted_range_axis(end, start, delete_start, count)
            .map(|(end, start)| (start, end));
    }

    if end < delete_start {
        return Some((start, end));
    }
    if start > delete_end {
        return Some((start.saturating_sub(count), end.saturating_sub(count)));
    }

    let keeps_before = start < delete_start;
    let keeps_after = end > delete_end;
    match (keeps_before, keeps_after) {
        (false, false) => None,
        (true, false) => Some((start, delete_start.saturating_sub(1))),
        (false, true) => Some((delete_start, end.saturating_sub(count))),
        (true, true) => Some((start, end.saturating_sub(count))),
    }
}

fn to_zero_based(value: u32) -> Option<usize> {
    usize::try_from(value.checked_sub(1)?).ok()
}

fn to_one_based(value: usize) -> Option<u32> {
    u32::try_from(value.checked_add(1)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjusts_formula_references_for_inserted_columns() {
        assert_eq!(
            adjust_formula_references(
                "SUM(Inputs!A1:B2)",
                "Inputs",
                "Other",
                StructureShift::InsertColumns {
                    col_index: 1,
                    count: 1,
                },
            ),
            "SUM(Inputs!A1:C2)"
        );
    }

    #[test]
    fn adjusts_formula_references_for_deleted_rows() {
        assert_eq!(
            adjust_formula_references(
                "Inputs!A1+Inputs!A2",
                "Inputs",
                "Other",
                StructureShift::DeleteRows {
                    row_index: 1,
                    count: 1,
                },
            ),
            "Inputs!A1 + #REF!"
        );
    }

    #[test]
    fn leaves_reference_like_text_literals_unchanged() {
        assert_eq!(
            adjust_formula_references(
                r#""Inputs!A1"&Inputs!A1"#,
                "Inputs",
                "Other",
                StructureShift::InsertRows {
                    row_index: 0,
                    count: 1,
                },
            ),
            r#""Inputs!A1" & Inputs!A2"#
        );
    }

    #[test]
    fn leaves_unparseable_formulas_unchanged() {
        assert_eq!(
            adjust_formula_references(
                r#"SOME_UNSUPPORTED_FUNC("Inputs!A1", )"#,
                "Inputs",
                "Other",
                StructureShift::InsertRows {
                    row_index: 0,
                    count: 1,
                },
            ),
            r#"SOME_UNSUPPORTED_FUNC("Inputs!A1", )"#
        );
    }

    #[test]
    fn shrinks_ranges_when_deleted_rows_touch_range_edges() {
        assert_eq!(
            adjust_formula_references(
                "SUM(Inputs!A1:A3)",
                "Inputs",
                "Other",
                StructureShift::DeleteRows {
                    row_index: 0,
                    count: 1,
                },
            ),
            "SUM(Inputs!A1:A2)"
        );

        assert_eq!(
            adjust_formula_references(
                "SUM(Inputs!A1:A3)",
                "Inputs",
                "Other",
                StructureShift::DeleteRows {
                    row_index: 2,
                    count: 1,
                },
            ),
            "SUM(Inputs!A1:A2)"
        );
    }

    #[test]
    fn shrinks_ranges_when_deleted_columns_touch_range_edges() {
        assert_eq!(
            adjust_formula_references(
                "SUM(Inputs!A1:C1)",
                "Inputs",
                "Other",
                StructureShift::DeleteColumns {
                    col_index: 0,
                    count: 1,
                },
            ),
            "SUM(Inputs!A1:B1)"
        );

        assert_eq!(
            adjust_formula_references(
                "SUM(Inputs!A1:C1)",
                "Inputs",
                "Other",
                StructureShift::DeleteColumns {
                    col_index: 2,
                    count: 1,
                },
            ),
            "SUM(Inputs!A1:B1)"
        );
    }

    #[test]
    fn removes_ranges_only_when_deleted_rows_cover_whole_range() {
        assert_eq!(
            adjust_formula_references(
                "SUM(Inputs!A1:A3)",
                "Inputs",
                "Other",
                StructureShift::DeleteRows {
                    row_index: 0,
                    count: 3,
                },
            ),
            "SUM(#REF!)"
        );
    }

    #[test]
    fn adjusts_formula_references_with_locked_coordinates_and_quoted_sheets() {
        assert_eq!(
            adjust_formula_references(
                "'Input Sheet'!$A$1:$B2",
                "Input Sheet",
                "Other",
                StructureShift::InsertRows {
                    row_index: 1,
                    count: 1,
                },
            ),
            "'Input Sheet'!$A$1:$B3"
        );
    }

    #[test]
    fn leaves_other_sheet_references_unchanged() {
        assert_eq!(
            adjust_formula_references(
                "Other!A1+Inputs!A1",
                "Inputs",
                "Current",
                StructureShift::InsertRows {
                    row_index: 0,
                    count: 1,
                },
            ),
            "Other!A1 + Inputs!A2"
        );
    }
}
