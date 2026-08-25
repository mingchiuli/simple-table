use formualizer_parse::parser::{ASTNodeType, ReferenceType};

use crate::formula::ast::{FormulaAstService, FormulaTextEdit, apply_formula_text_edits};
use crate::formula::sheet_name::sheet_names_equal;

#[derive(Clone, Copy)]
pub enum StructureShift {
    InsertRows { row_index: usize, count: usize },
    DeleteRows { row_index: usize, count: usize },
    InsertColumns { col_index: usize, count: usize },
    DeleteColumns { col_index: usize, count: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormulaReferenceRewrite {
    pub formula: String,
    pub skipped: bool,
}

pub(crate) fn translate_formula_for_move(
    ast_service: &mut FormulaAstService,
    formula: &str,
    row_delta: isize,
    col_delta: isize,
) -> Result<String, String> {
    let parsed = ast_service.parse(formula)?;
    let mut edits = Vec::new();
    let mut reference_nodes = Vec::new();
    parsed.collect_reference_nodes(&mut reference_nodes);
    for node in reference_nodes {
        let ASTNodeType::Reference { reference, .. } = &node.node_type else {
            continue;
        };
        let translated = translate_reference_for_move(reference, row_delta, col_delta)?;
        let token = node
            .source_token
            .as_ref()
            .ok_or_else(|| "formula reference has no source location".to_string())?;
        let (start, end) = parsed
            .source()
            .original_span(token.start, token.end)
            .ok_or_else(|| "formula reference source location is invalid".to_string())?;
        edits.push(FormulaTextEdit {
            start,
            end,
            replacement: translated.normalise(),
        });
    }
    apply_formula_text_edits(formula, edits)
        .ok_or_else(|| "formula reference edits overlap".to_string())
}

fn translate_reference_for_move(
    reference: &ReferenceType,
    row_delta: isize,
    col_delta: isize,
) -> Result<ReferenceType, String> {
    match reference {
        ReferenceType::Cell {
            sheet,
            row,
            col,
            row_abs,
            col_abs,
        } => Ok(ReferenceType::Cell {
            sheet: sheet.clone(),
            row: translate_axis(*row, *row_abs, row_delta)?,
            col: translate_axis(*col, *col_abs, col_delta)?,
            row_abs: *row_abs,
            col_abs: *col_abs,
        }),
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
        } => Ok(ReferenceType::Range {
            sheet: sheet.clone(),
            start_row: translate_optional_axis(*start_row, *start_row_abs, row_delta)?,
            start_col: translate_optional_axis(*start_col, *start_col_abs, col_delta)?,
            end_row: translate_optional_axis(*end_row, *end_row_abs, row_delta)?,
            end_col: translate_optional_axis(*end_col, *end_col_abs, col_delta)?,
            start_row_abs: *start_row_abs,
            start_col_abs: *start_col_abs,
            end_row_abs: *end_row_abs,
            end_col_abs: *end_col_abs,
        }),
        ReferenceType::Cell3D { .. } => {
            Err("3D cell references are not supported by sort".to_string())
        }
        ReferenceType::Range3D { .. } => {
            Err("3D range references are not supported by sort".to_string())
        }
        ReferenceType::External(_) => {
            Err("external references are not supported by sort".to_string())
        }
        ReferenceType::Table(_) => Err("table references are not supported by sort".to_string()),
        ReferenceType::NamedRange(_) => Err("named ranges are not supported by sort".to_string()),
    }
}

fn translate_optional_axis(
    value: Option<u32>,
    absolute: bool,
    delta: isize,
) -> Result<Option<u32>, String> {
    value
        .map(|value| translate_axis(value, absolute, delta))
        .transpose()
}

fn translate_axis(value: u32, absolute: bool, delta: isize) -> Result<u32, String> {
    if absolute || delta == 0 {
        return Ok(value);
    }
    let value = isize::try_from(value).map_err(|_| "formula reference is too large")?;
    let translated = value
        .checked_add(delta)
        .filter(|value| *value >= 1)
        .ok_or_else(|| "formula movement would create an invalid reference".to_string())?;
    u32::try_from(translated)
        .map_err(|_| "formula reference exceeds the supported range".to_string())
}

pub fn adjust_formula_references(
    ast_service: &mut FormulaAstService,
    formula: &str,
    target_sheet_name: &str,
    current_sheet_name: &str,
    shift: StructureShift,
) -> FormulaReferenceRewrite {
    rewrite_formula_references(
        ast_service,
        formula,
        target_sheet_name,
        current_sheet_name,
        ReferenceRewrite::Shift(shift),
        ReferenceMatchMode::AllTargetReferences,
    )
}

pub fn adjust_explicit_sheet_name_case_mismatched_references(
    ast_service: &mut FormulaAstService,
    formula: &str,
    target_sheet_name: &str,
    current_sheet_name: &str,
    shift: StructureShift,
) -> FormulaReferenceRewrite {
    rewrite_formula_references(
        ast_service,
        formula,
        target_sheet_name,
        current_sheet_name,
        ReferenceRewrite::Shift(shift),
        ReferenceMatchMode::ExplicitSheetNameCaseMismatch,
    )
}

pub fn invalidate_deleted_sheet_references(
    ast_service: &mut FormulaAstService,
    formula: &str,
    deleted_sheet_name: &str,
    current_sheet_name: &str,
) -> FormulaReferenceRewrite {
    rewrite_formula_references(
        ast_service,
        formula,
        deleted_sheet_name,
        current_sheet_name,
        ReferenceRewrite::DeletedSheet,
        ReferenceMatchMode::AllTargetReferences,
    )
}

#[derive(Clone, Copy)]
enum ReferenceRewrite {
    Shift(StructureShift),
    DeletedSheet,
}

#[derive(Clone, Copy)]
enum ReferenceMatchMode {
    AllTargetReferences,
    ExplicitSheetNameCaseMismatch,
}

fn rewrite_formula_references(
    ast_service: &mut FormulaAstService,
    formula: &str,
    target_sheet_name: &str,
    current_sheet_name: &str,
    rewrite: ReferenceRewrite,
    match_mode: ReferenceMatchMode,
) -> FormulaReferenceRewrite {
    let parsed = match ast_service.parse(formula) {
        Ok(parsed) => parsed,
        Err(_) => {
            return FormulaReferenceRewrite {
                formula: formula.to_string(),
                skipped: true,
            };
        }
    };

    let mut edits = Vec::new();
    let mut reference_nodes = Vec::new();
    parsed.collect_reference_nodes(&mut reference_nodes);
    for node in reference_nodes {
        let ASTNodeType::Reference { reference, .. } = &node.node_type else {
            continue;
        };
        if !reference_matches_target_sheet(
            reference,
            target_sheet_name,
            current_sheet_name,
            match_mode,
        ) {
            continue;
        }
        let Some(token) = node.source_token.as_ref() else {
            continue;
        };
        let Some((start, end)) = parsed.source().original_span(token.start, token.end) else {
            continue;
        };
        let replacement = match rewrite {
            ReferenceRewrite::Shift(shift) => adjust_reference(reference.clone(), shift)
                .map(|reference| reference.normalise())
                .unwrap_or_else(|| "#REF!".to_string()),
            ReferenceRewrite::DeletedSheet => "#REF!".to_string(),
        };
        edits.push(FormulaTextEdit {
            start,
            end,
            replacement,
        });
    }
    if edits.is_empty() {
        return FormulaReferenceRewrite {
            formula: formula.to_string(),
            skipped: false,
        };
    }

    FormulaReferenceRewrite {
        formula: apply_formula_text_edits(formula, edits).unwrap_or_else(|| formula.to_string()),
        skipped: false,
    }
}

fn reference_matches_target_sheet(
    reference: &ReferenceType,
    target_sheet_name: &str,
    current_sheet_name: &str,
    match_mode: ReferenceMatchMode,
) -> bool {
    if matches!(
        match_mode,
        ReferenceMatchMode::ExplicitSheetNameCaseMismatch
    ) {
        return reference_has_case_mismatched_explicit_target_sheet(reference, target_sheet_name);
    }

    match reference {
        ReferenceType::Cell { sheet, .. } | ReferenceType::Range { sheet, .. } => sheet
            .as_deref()
            .map(|name| sheet_names_equal(name, target_sheet_name))
            .unwrap_or_else(|| sheet_names_equal(current_sheet_name, target_sheet_name)),
        ReferenceType::Cell3D {
            sheet_first,
            sheet_last,
            ..
        }
        | ReferenceType::Range3D {
            sheet_first,
            sheet_last,
            ..
        } => {
            sheet_names_equal(sheet_first, target_sheet_name)
                || sheet_names_equal(sheet_last, target_sheet_name)
        }
        ReferenceType::External(_) | ReferenceType::Table(_) | ReferenceType::NamedRange(_) => {
            false
        }
    }
}

fn reference_has_case_mismatched_explicit_target_sheet(
    reference: &ReferenceType,
    target_sheet_name: &str,
) -> bool {
    match reference {
        ReferenceType::Cell { sheet, .. } | ReferenceType::Range { sheet, .. } => {
            sheet.as_deref().is_some_and(|name| {
                sheet_names_equal(name, target_sheet_name) && name != target_sheet_name
            })
        }
        ReferenceType::Cell3D {
            sheet_first,
            sheet_last,
            ..
        }
        | ReferenceType::Range3D {
            sheet_first,
            sheet_last,
            ..
        } => {
            (sheet_names_equal(sheet_first, target_sheet_name) && sheet_first != target_sheet_name)
                || (sheet_names_equal(sheet_last, target_sheet_name)
                    && sheet_last != target_sheet_name)
        }
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
    use crate::formula::ast::FormulaAstService;

    fn rewritten(outcome: FormulaReferenceRewrite) -> String {
        assert!(!outcome.skipped);
        outcome.formula
    }

    fn adjusted(
        formula: &str,
        target_sheet_name: &str,
        current_sheet_name: &str,
        shift: StructureShift,
    ) -> FormulaReferenceRewrite {
        let mut ast_service = FormulaAstService::new();
        adjust_formula_references(
            &mut ast_service,
            formula,
            target_sheet_name,
            current_sheet_name,
            shift,
        )
    }

    fn invalidated(
        formula: &str,
        deleted_sheet_name: &str,
        current_sheet_name: &str,
    ) -> FormulaReferenceRewrite {
        let mut ast_service = FormulaAstService::new();
        invalidate_deleted_sheet_references(
            &mut ast_service,
            formula,
            deleted_sheet_name,
            current_sheet_name,
        )
    }

    fn adjusted_case_mismatch(
        formula: &str,
        target_sheet_name: &str,
        current_sheet_name: &str,
        shift: StructureShift,
    ) -> FormulaReferenceRewrite {
        let mut ast_service = FormulaAstService::new();
        adjust_explicit_sheet_name_case_mismatched_references(
            &mut ast_service,
            formula,
            target_sheet_name,
            current_sheet_name,
            shift,
        )
    }

    #[test]
    fn adjusts_formula_references_for_inserted_columns() {
        assert_eq!(
            rewritten(adjusted(
                "SUM(Inputs!A1:B2)",
                "Inputs",
                "Other",
                StructureShift::InsertColumns {
                    col_index: 1,
                    count: 1,
                },
            )),
            "SUM(Inputs!A1:C2)"
        );
    }

    #[test]
    fn adjusts_formula_references_for_deleted_rows() {
        assert_eq!(
            rewritten(adjusted(
                "Inputs!A1+Inputs!A2",
                "Inputs",
                "Other",
                StructureShift::DeleteRows {
                    row_index: 1,
                    count: 1,
                },
            )),
            "Inputs!A1+#REF!"
        );
    }

    #[test]
    fn leaves_reference_like_text_literals_unchanged() {
        assert_eq!(
            rewritten(adjusted(
                r#""Inputs!A1"&Inputs!A1"#,
                "Inputs",
                "Other",
                StructureShift::InsertRows {
                    row_index: 0,
                    count: 1,
                },
            )),
            r#""Inputs!A1"&Inputs!A2"#
        );
    }

    #[test]
    fn leaves_unparseable_formulas_unchanged() {
        let outcome = adjusted(
            "SUM(",
            "Inputs",
            "Other",
            StructureShift::InsertRows {
                row_index: 0,
                count: 1,
            },
        );
        assert!(outcome.skipped);
        assert_eq!(outcome.formula, "SUM(");
    }

    #[test]
    fn shrinks_ranges_when_deleted_rows_touch_range_edges() {
        assert_eq!(
            rewritten(adjusted(
                "SUM(Inputs!A1:A3)",
                "Inputs",
                "Other",
                StructureShift::DeleteRows {
                    row_index: 0,
                    count: 1,
                },
            )),
            "SUM(Inputs!A1:A2)"
        );

        assert_eq!(
            rewritten(adjusted(
                "SUM(Inputs!A1:A3)",
                "Inputs",
                "Other",
                StructureShift::DeleteRows {
                    row_index: 2,
                    count: 1,
                },
            )),
            "SUM(Inputs!A1:A2)"
        );
    }

    #[test]
    fn shrinks_ranges_when_deleted_columns_touch_range_edges() {
        assert_eq!(
            rewritten(adjusted(
                "SUM(Inputs!A1:C1)",
                "Inputs",
                "Other",
                StructureShift::DeleteColumns {
                    col_index: 0,
                    count: 1,
                },
            )),
            "SUM(Inputs!A1:B1)"
        );

        assert_eq!(
            rewritten(adjusted(
                "SUM(Inputs!A1:C1)",
                "Inputs",
                "Other",
                StructureShift::DeleteColumns {
                    col_index: 2,
                    count: 1,
                },
            )),
            "SUM(Inputs!A1:B1)"
        );
    }

    #[test]
    fn removes_ranges_only_when_deleted_rows_cover_whole_range() {
        assert_eq!(
            rewritten(adjusted(
                "SUM(Inputs!A1:A3)",
                "Inputs",
                "Other",
                StructureShift::DeleteRows {
                    row_index: 0,
                    count: 3,
                },
            )),
            "SUM(#REF!)"
        );
    }

    #[test]
    fn adjusts_formula_references_with_locked_coordinates_and_quoted_sheets() {
        assert_eq!(
            rewritten(adjusted(
                "'Input Sheet'!$A$1:$B2",
                "Input Sheet",
                "Other",
                StructureShift::InsertRows {
                    row_index: 1,
                    count: 1,
                },
            )),
            "'Input Sheet'!$A$1:$B3"
        );
    }

    #[test]
    fn leaves_other_sheet_references_unchanged() {
        assert_eq!(
            rewritten(adjusted(
                "Other!A1+Inputs!A1",
                "Inputs",
                "Current",
                StructureShift::InsertRows {
                    row_index: 0,
                    count: 1,
                },
            )),
            "Other!A1+Inputs!A2"
        );
    }

    #[test]
    fn matches_sheet_names_case_insensitively() {
        assert_eq!(
            rewritten(adjusted(
                "inputs!A1",
                "Inputs",
                "Other",
                StructureShift::InsertRows {
                    row_index: 0,
                    count: 1,
                },
            )),
            "inputs!A2"
        );

        assert_eq!(
            rewritten(adjusted(
                "A1",
                "Inputs",
                "inputs",
                StructureShift::InsertRows {
                    row_index: 0,
                    count: 1,
                },
            )),
            "A2"
        );

        assert_eq!(
            rewritten(adjusted(
                "sheet1:sheet3!A1",
                "Sheet1",
                "Other",
                StructureShift::InsertRows {
                    row_index: 0,
                    count: 1,
                },
            )),
            "sheet1:sheet3!A2"
        );
    }

    #[test]
    fn adjusts_only_explicit_case_mismatched_current_sheet_references() {
        assert_eq!(
            rewritten(adjusted_case_mismatch(
                "A2+Inputs!A2+inputs!A2",
                "Inputs",
                "Inputs",
                StructureShift::InsertRows {
                    row_index: 0,
                    count: 1,
                },
            )),
            "A2+Inputs!A2+inputs!A3"
        );
    }

    #[test]
    fn preserves_formula_text_outside_rewritten_reference_tokens() {
        assert_eq!(
            rewritten(adjusted(
                "=sum(  Inputs!a1 , \"Inputs!A1\" , Other!A1 )",
                "Inputs",
                "Other",
                StructureShift::InsertRows {
                    row_index: 0,
                    count: 1,
                },
            )),
            "=sum(  Inputs!A2 , \"Inputs!A1\" , Other!A1 )"
        );
    }

    #[test]
    fn invalidates_deleted_sheet_without_reformatting_formula() {
        assert_eq!(
            rewritten(invalidated(
                "=if( Inputs!A1>0 , \"Inputs!A1\" , Other!A1 )",
                "Inputs",
                "Other",
            )),
            "=if( #REF!>0 , \"Inputs!A1\" , Other!A1 )"
        );
    }

    #[test]
    fn translates_relative_formula_references_when_a_sorted_row_moves() {
        let mut ast_service = FormulaAstService::new();

        let translated =
            translate_formula_for_move(&mut ast_service, "=A2+$B$1+C$3+$D4+SUM(A2:B3)", 2, 1)
                .expect("translate formula");

        assert_eq!(translated, "=B4+$B$1+D$3+$D6+SUM(B4:C5)");
    }

    #[test]
    fn rejects_formula_moves_that_cross_the_sheet_origin() {
        let mut ast_service = FormulaAstService::new();

        let error = translate_formula_for_move(&mut ast_service, "=A1", -1, 0)
            .expect_err("relative reference would become invalid");

        assert!(error.contains("invalid reference"));
    }
}
