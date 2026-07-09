use std::collections::HashMap;

use formualizer_parse::parser::{ASTNodeType, ReferenceType};

use crate::formula::ast::{FormulaAstService, FormulaTextEdit, apply_formula_text_edits};

pub(crate) fn sheet_name_key(name: &str) -> String {
    name.to_lowercase()
}

pub(crate) fn sheet_names_equal(left: &str, right: &str) -> bool {
    left == right || sheet_name_key(left) == sheet_name_key(right)
}

pub(crate) fn canonicalize_formula_sheet_names(
    ast_service: &mut FormulaAstService,
    formula: &str,
    sheet_names: &[String],
) -> Result<String, String> {
    let canonical_names = canonical_sheet_names(sheet_names);
    if canonical_names.is_empty() {
        return Ok(formula.to_string());
    }

    let parsed = ast_service.parse(formula)?;
    let mut edits = Vec::new();
    let mut reference_nodes = Vec::new();
    parsed.collect_reference_nodes(&mut reference_nodes);
    for node in reference_nodes {
        let ASTNodeType::Reference { reference, .. } = &node.node_type else {
            continue;
        };
        let canonical = canonicalize_reference_sheet_names(reference, &canonical_names);
        if canonical == *reference {
            continue;
        }
        let Some(token) = node.source_token.as_ref() else {
            continue;
        };
        let Some((start, end)) = parsed.source().original_span(token.start, token.end) else {
            continue;
        };
        edits.push(FormulaTextEdit {
            start,
            end,
            replacement: canonical.normalise(),
        });
    }

    Ok(apply_formula_text_edits(formula, edits).unwrap_or_else(|| formula.to_string()))
}

fn canonical_sheet_names(sheet_names: &[String]) -> HashMap<String, &str> {
    let mut canonical_names = HashMap::new();
    for sheet_name in sheet_names {
        canonical_names
            .entry(sheet_name_key(sheet_name))
            .or_insert(sheet_name.as_str());
    }
    canonical_names
}

fn canonicalize_reference_sheet_names(
    reference: &ReferenceType,
    canonical_names: &HashMap<String, &str>,
) -> ReferenceType {
    match reference {
        ReferenceType::Cell {
            sheet,
            row,
            col,
            row_abs,
            col_abs,
        } => ReferenceType::Cell {
            sheet: sheet
                .as_deref()
                .map(|sheet| canonical_sheet_name(sheet, canonical_names).to_string()),
            row: *row,
            col: *col,
            row_abs: *row_abs,
            col_abs: *col_abs,
        },
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
        } => ReferenceType::Range {
            sheet: sheet
                .as_deref()
                .map(|sheet| canonical_sheet_name(sheet, canonical_names).to_string()),
            start_row: *start_row,
            start_col: *start_col,
            end_row: *end_row,
            end_col: *end_col,
            start_row_abs: *start_row_abs,
            start_col_abs: *start_col_abs,
            end_row_abs: *end_row_abs,
            end_col_abs: *end_col_abs,
        },
        ReferenceType::Cell3D {
            sheet_first,
            sheet_last,
            row,
            col,
            row_abs,
            col_abs,
        } => ReferenceType::Cell3D {
            sheet_first: canonical_sheet_name(sheet_first, canonical_names).to_string(),
            sheet_last: canonical_sheet_name(sheet_last, canonical_names).to_string(),
            row: *row,
            col: *col,
            row_abs: *row_abs,
            col_abs: *col_abs,
        },
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
        } => ReferenceType::Range3D {
            sheet_first: canonical_sheet_name(sheet_first, canonical_names).to_string(),
            sheet_last: canonical_sheet_name(sheet_last, canonical_names).to_string(),
            start_row: *start_row,
            start_col: *start_col,
            end_row: *end_row,
            end_col: *end_col,
            start_row_abs: *start_row_abs,
            start_col_abs: *start_col_abs,
            end_row_abs: *end_row_abs,
            end_col_abs: *end_col_abs,
        },
        ReferenceType::External(_) | ReferenceType::Table(_) | ReferenceType::NamedRange(_) => {
            reference.clone()
        }
    }
}

fn canonical_sheet_name<'a>(
    sheet_name: &'a str,
    canonical_names: &HashMap<String, &'a str>,
) -> &'a str {
    canonical_names
        .get(&sheet_name_key(sheet_name))
        .copied()
        .unwrap_or(sheet_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canonicalized(formula: &str, sheet_names: &[&str]) -> String {
        let mut ast_service = FormulaAstService::new();
        let sheet_names = sheet_names
            .iter()
            .map(|sheet_name| sheet_name.to_string())
            .collect::<Vec<_>>();
        canonicalize_formula_sheet_names(&mut ast_service, formula, &sheet_names)
            .expect("canonicalize formula sheet names")
    }

    #[test]
    fn canonicalizes_known_sheet_names_case_insensitively() {
        assert_eq!(
            canonicalized("inputs!A1+summary!B2", &["Inputs", "Summary"]),
            "Inputs!A1+Summary!B2"
        );
    }

    #[test]
    fn leaves_string_literals_and_unknown_sheet_names_unchanged() {
        assert_eq!(
            canonicalized(r#""inputs!A1"&unknown!A1&inputs!A1"#, &["Inputs"]),
            r#""inputs!A1"&unknown!A1&Inputs!A1"#
        );
    }

    #[test]
    fn canonicalizes_sheet_names_when_formula_has_leading_equals() {
        assert_eq!(canonicalized("=inputs!A1", &["Inputs"]), "=Inputs!A1");
    }
}
