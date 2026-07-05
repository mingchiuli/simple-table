use std::collections::HashSet;

use formualizer_workbook::Workbook;

use crate::error::AppError;
use crate::formula::ast::FormulaAstService;
use crate::formula::cell_ref::FormulaCellRef;
use crate::formula::value_codec::{cell_to_literal, to_formula_index};
use crate::types::{CellValue, FileData, SheetCellChange};

#[derive(Default)]
pub(crate) struct FormulaRegistrationResult {
    pub(crate) registered_formulas: HashSet<FormulaCellRef>,
    pub(crate) invalid_formulas: Vec<SheetCellChange>,
}

pub(crate) fn register_workbook_cells(
    workbook: &mut Workbook,
    ast_service: &mut FormulaAstService,
    file_data: &mut FileData,
) -> Result<FormulaRegistrationResult, AppError> {
    let mut result = FormulaRegistrationResult::default();

    for (sheet_index, sheet) in file_data.sheets.iter_mut().enumerate() {
        for (row_idx, row) in sheet.rows.iter_mut().enumerate() {
            for (col_idx, cell) in row.iter_mut().enumerate() {
                let cell_result = set_workbook_cell(
                    workbook,
                    ast_service,
                    &sheet.name,
                    sheet_index,
                    row_idx,
                    col_idx,
                    cell,
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
    sheet_name: &str,
    sheet_index: usize,
    row_idx: usize,
    col_idx: usize,
    cell: &CellValue,
) -> Result<FormulaRegistrationResult, AppError> {
    let mut result = FormulaRegistrationResult::default();
    let row = to_formula_index(row_idx);
    let col = to_formula_index(col_idx);
    match cell {
        CellValue::Formula { formula, .. } => {
            match ast_service.validate(formula).and_then(|_| {
                workbook
                    .set_formula(sheet_name, row, col, formula)
                    .map_err(|error| error.to_string())
            }) {
                Ok(()) => {
                    result.registered_formulas.insert(FormulaCellRef {
                        sheet_index,
                        row: row_idx,
                        col: col_idx,
                    });
                }
                Err(error) => {
                    workbook
                        .set_value(
                            sheet_name,
                            row,
                            col,
                            formualizer_workbook::LiteralValue::Empty,
                        )
                        .map_err(|error| AppError::Internal(error.to_string()))?;
                    let value = cell.with_formula_result(CellValue::Null, Some(error));
                    result.invalid_formulas.push(SheetCellChange::new(
                        sheet_index,
                        row_idx,
                        col_idx,
                        value,
                    ));
                }
            }
        }
        _ => workbook
            .set_value(sheet_name, row, col, cell_to_literal(cell))
            .map_err(|error| AppError::Internal(error.to_string()))?,
    }

    Ok(result)
}

pub(crate) fn apply_cell_changes(file_data: &mut FileData, changes: &[SheetCellChange]) {
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
