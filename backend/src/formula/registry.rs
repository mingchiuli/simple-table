use crate::document_data::DocumentData;
use std::collections::HashSet;

use formualizer_workbook::Workbook;

use crate::domain::{CellValue, DocumentCellChange};
use crate::error::AppError;
use crate::formula::ast::FormulaAstService;
use crate::formula::cell_ref::FormulaCellRef;
use crate::formula::sheet_name::canonicalize_formula_sheet_names;
use crate::formula::value_codec::{cell_to_literal, to_formula_index};

#[derive(Default)]
pub(crate) struct FormulaRegistrationResult {
    pub(crate) registered_formulas: HashSet<FormulaCellRef>,
    pub(crate) invalid_formulas: Vec<DocumentCellChange>,
}

pub(crate) struct FormulaCellRegistration<'a> {
    pub(crate) sheet_name: &'a str,
    pub(crate) cell_ref: FormulaCellRef,
    pub(crate) cell: &'a CellValue,
    pub(crate) sheet_names: &'a [String],
}

pub(crate) fn register_workbook_cells(
    workbook: &mut Workbook,
    ast_service: &mut FormulaAstService,
    file_data: &mut DocumentData,
) -> Result<FormulaRegistrationResult, AppError> {
    let mut result = FormulaRegistrationResult::default();
    let sheet_names: Vec<String> = file_data
        .sheets
        .iter()
        .map(|sheet| sheet.name.clone())
        .collect();

    for (sheet_index, sheet) in file_data.sheets.iter_mut().enumerate() {
        for (row_idx, row) in sheet.rows.iter_mut().enumerate() {
            for (col_idx, cell) in row.iter_mut().enumerate() {
                let cell_result = set_workbook_cell(
                    workbook,
                    ast_service,
                    FormulaCellRegistration {
                        sheet_name: &sheet.name,
                        cell_ref: FormulaCellRef {
                            sheet_index,
                            row: row_idx,
                            col: col_idx,
                        },
                        cell,
                        sheet_names: &sheet_names,
                    },
                )?;
                result
                    .registered_formulas
                    .extend(cell_result.registered_formulas);
                result.invalid_formulas.extend(cell_result.invalid_formulas);
            }
        }
    }

    Ok(result)
}

pub(crate) fn set_workbook_cell(
    workbook: &mut Workbook,
    ast_service: &mut FormulaAstService,
    registration: FormulaCellRegistration<'_>,
) -> Result<FormulaRegistrationResult, AppError> {
    let mut result = FormulaRegistrationResult::default();
    let row = to_formula_index(registration.cell_ref.row);
    let col = to_formula_index(registration.cell_ref.col);
    match registration.cell {
        CellValue::Formula { formula, .. } => {
            match canonicalize_formula_sheet_names(ast_service, formula, registration.sheet_names)
                .and_then(|formula| {
                    workbook
                        .set_formula(registration.sheet_name, row, col, &formula)
                        .map_err(|error| error.to_string())
                }) {
                Ok(()) => {
                    result.registered_formulas.insert(registration.cell_ref);
                }
                Err(error) => {
                    workbook
                        .set_value(
                            registration.sheet_name,
                            row,
                            col,
                            formualizer_workbook::LiteralValue::Empty,
                        )
                        .map_err(|error| AppError::Internal(error.to_string()))?;
                    let value = registration
                        .cell
                        .with_formula_result(CellValue::Null, Some(error));
                    result.invalid_formulas.push(DocumentCellChange::new(
                        registration.cell_ref.sheet_index,
                        registration.cell_ref.row,
                        registration.cell_ref.col,
                        value,
                    ));
                }
            }
        }
        _ => workbook
            .set_value(
                registration.sheet_name,
                row,
                col,
                cell_to_literal(registration.cell),
            )
            .map_err(|error| AppError::Internal(error.to_string()))?,
    }

    Ok(result)
}

pub(crate) fn apply_cell_changes(file_data: &mut DocumentData, changes: &[DocumentCellChange]) {
    for change in changes {
        let Some(cell) = file_data
            .sheets
            .get_mut(change.sheet_index)
            .and_then(|sheet| sheet.rows.get_mut(change.row))
            .and_then(|row| row.get_mut(change.col))
        else {
            continue;
        };
        *cell = change.value.clone();
    }
}
