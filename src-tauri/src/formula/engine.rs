use std::collections::{HashMap, HashSet, VecDeque};

use formualizer_parse::parser::{CollectPolicy, ReferenceType};
use formualizer_workbook::{LiteralValue, Workbook, WorkbookMode};
use serde_json::Value;

use crate::error::AppError;
use crate::types::{CellValue, FileData, FormulaDiagnostics, SheetCellChange};

const MAX_RANGE_DEPENDENCY_CELLS: u32 = 10_000;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct FormulaCellRef {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
}

#[derive(Default)]
struct FormulaDependencyIndex {
    formulas: HashSet<FormulaCellRef>,
    dependents_by_source: HashMap<FormulaCellRef, HashSet<FormulaCellRef>>,
    always_recalculate: HashSet<FormulaCellRef>,
    diagnostics: FormulaDiagnostics,
}

#[derive(Default)]
struct FormulaRegistrationResult {
    registered_formulas: HashSet<FormulaCellRef>,
    invalid_formulas: Vec<SheetCellChange>,
}

pub struct FormulaRuntime {
    workbook: Workbook,
    dependency_index: FormulaDependencyIndex,
    registered_formulas: HashSet<FormulaCellRef>,
}

pub struct FormulaRebuildResult {
    pub changes: Vec<SheetCellChange>,
    pub diagnostics: FormulaDiagnostics,
}

impl FormulaRuntime {
    pub fn new(file_data: &mut FileData) -> Result<Self, AppError> {
        let mut runtime = Self::empty();
        runtime.rebuild(file_data)?;
        Ok(runtime)
    }

    pub fn empty() -> Self {
        Self {
            workbook: Workbook::new_with_mode(WorkbookMode::Ephemeral),
            dependency_index: FormulaDependencyIndex::default(),
            registered_formulas: HashSet::new(),
        }
    }

    pub fn rebuild(&mut self, file_data: &mut FileData) -> Result<Vec<SheetCellChange>, AppError> {
        Ok(self.rebuild_with_diagnostics(file_data)?.changes)
    }

    pub fn rebuild_with_diagnostics(
        &mut self,
        file_data: &mut FileData,
    ) -> Result<FormulaRebuildResult, AppError> {
        let mut workbook = Workbook::new_with_mode(WorkbookMode::Ephemeral);
        for sheet in &file_data.sheets {
            workbook
                .add_sheet(&sheet.name)
                .map_err(|error| AppError::Internal(error.to_string()))?;
        }

        let mut registration_result = register_workbook_cells(&mut workbook, file_data)?;
        let mut changes = std::mem::take(&mut registration_result.invalid_formulas);

        let sheet_names: Vec<String> = file_data
            .sheets
            .iter()
            .map(|sheet| sheet.name.clone())
            .collect();
        workbook
            .prepare_graph_for_sheets(sheet_names.iter().map(String::as_str))
            .map_err(|error| AppError::Internal(error.to_string()))?;

        apply_cell_changes(file_data, &changes);
        self.workbook = workbook;
        self.registered_formulas = std::mem::take(&mut registration_result.registered_formulas);
        self.dependency_index = build_dependency_index(file_data, &self.registered_formulas);
        self.refresh_formula_diagnostics(file_data);
        changes.extend(self.recalculate_all_formula_cells(file_data)?);
        Ok(FormulaRebuildResult {
            changes,
            diagnostics: self.dependency_index.diagnostics.clone(),
        })
    }

    pub fn diagnostics(&self) -> FormulaDiagnostics {
        self.dependency_index.diagnostics.clone()
    }

    pub fn sync_cell_and_recalculate(
        &mut self,
        file_data: &mut FileData,
        sheet_index: usize,
        row: usize,
        col: usize,
    ) -> Result<Vec<SheetCellChange>, AppError> {
        let sheet = file_data
            .sheets
            .get(sheet_index)
            .ok_or(AppError::InvalidSheetIndex(sheet_index))?;
        let cell_ref = FormulaCellRef {
            sheet_index,
            row,
            col,
        };
        let cell = sheet
            .rows
            .get(row)
            .and_then(|row_data| row_data.get(col))
            .ok_or(AppError::InvalidCellPosition { row, col })?;
        let was_formula = self.dependency_index.formulas.contains(&cell_ref);
        let is_formula = matches!(cell, CellValue::Formula { .. });

        let registration_result =
            set_workbook_cell(&mut self.workbook, &sheet.name, sheet_index, row, col, cell)?;
        let mut changes = registration_result.invalid_formulas;
        if registration_result.registered_formulas.contains(&cell_ref) {
            self.registered_formulas.insert(cell_ref.clone());
        } else {
            self.registered_formulas.remove(&cell_ref);
        }
        apply_cell_changes(file_data, &changes);

        if was_formula || is_formula {
            self.dependency_index = build_dependency_index(file_data, &self.registered_formulas);
        }
        self.refresh_formula_diagnostics(file_data);

        let mut targets = self.impacted_formula_cells(&cell_ref);
        if self.dependency_index.formulas.contains(&cell_ref) {
            targets.insert(cell_ref);
        }
        changes.extend(self.recalculate_formula_cells(file_data, targets.iter())?);
        Ok(changes)
    }

    pub fn sync_cells_and_recalculate(
        &mut self,
        file_data: &mut FileData,
        changed_cells: impl IntoIterator<Item = FormulaCellRef>,
    ) -> Result<Vec<SheetCellChange>, AppError> {
        let changed_cells: Vec<FormulaCellRef> = changed_cells.into_iter().collect();
        let mut changes = Vec::new();
        let mut dependency_graph_needs_rebuild = false;

        for cell_ref in &changed_cells {
            let sheet = file_data
                .sheets
                .get(cell_ref.sheet_index)
                .ok_or(AppError::InvalidSheetIndex(cell_ref.sheet_index))?;
            let cell = sheet
                .rows
                .get(cell_ref.row)
                .and_then(|row_data| row_data.get(cell_ref.col))
                .ok_or(AppError::InvalidCellPosition {
                    row: cell_ref.row,
                    col: cell_ref.col,
                })?;
            let was_formula = self.dependency_index.formulas.contains(cell_ref);
            let is_formula = matches!(cell, CellValue::Formula { .. });

            let registration_result = set_workbook_cell(
                &mut self.workbook,
                &sheet.name,
                cell_ref.sheet_index,
                cell_ref.row,
                cell_ref.col,
                cell,
            )?;
            changes.extend(registration_result.invalid_formulas);
            if registration_result.registered_formulas.contains(cell_ref) {
                self.registered_formulas.insert(*cell_ref);
            } else {
                self.registered_formulas.remove(cell_ref);
            }
            dependency_graph_needs_rebuild |= was_formula || is_formula;
        }

        apply_cell_changes(file_data, &changes);

        if dependency_graph_needs_rebuild {
            self.dependency_index = build_dependency_index(file_data, &self.registered_formulas);
        }
        self.refresh_formula_diagnostics(file_data);

        let mut targets = HashSet::new();
        for cell_ref in &changed_cells {
            targets.extend(self.impacted_formula_cells(cell_ref));
            if self.dependency_index.formulas.contains(cell_ref) {
                targets.insert(*cell_ref);
            }
        }
        changes.extend(self.recalculate_formula_cells(file_data, targets.iter())?);
        Ok(changes)
    }

    pub fn impacted_formula_cells_for(
        &self,
        changed_cells: impl IntoIterator<Item = FormulaCellRef>,
    ) -> Vec<FormulaCellRef> {
        let mut impacted = HashSet::new();
        for cell_ref in changed_cells {
            impacted.extend(self.impacted_formula_cells(&cell_ref));
            if self.dependency_index.formulas.contains(&cell_ref) {
                impacted.insert(cell_ref);
            }
        }
        impacted.into_iter().collect()
    }

    pub fn all_formula_cells(&self) -> Vec<FormulaCellRef> {
        self.dependency_index.formulas.iter().copied().collect()
    }

    fn refresh_formula_diagnostics(&mut self, file_data: &FileData) {
        self.dependency_index.diagnostics.invalid_formula_count =
            count_unregistered_formula_cells(file_data, &self.registered_formulas);
    }

    fn impacted_formula_cells(&self, changed_cell: &FormulaCellRef) -> HashSet<FormulaCellRef> {
        let mut impacted = self.dependency_index.always_recalculate.clone();
        let mut queue = VecDeque::new();
        queue.push_back(*changed_cell);
        queue.extend(impacted.iter().copied());

        while let Some(source) = queue.pop_front() {
            let Some(dependents) = self.dependency_index.dependents_by_source.get(&source) else {
                continue;
            };
            for dependent in dependents {
                if impacted.insert(*dependent) {
                    queue.push_back(*dependent);
                }
            }
        }

        impacted
    }

    fn recalculate_all_formula_cells(
        &mut self,
        file_data: &mut FileData,
    ) -> Result<Vec<SheetCellChange>, AppError> {
        let targets: Vec<FormulaCellRef> = self.dependency_index.formulas.iter().copied().collect();
        self.recalculate_formula_cells(file_data, targets.iter())
    }

    fn recalculate_formula_cells<'a>(
        &mut self,
        file_data: &mut FileData,
        targets: impl IntoIterator<Item = &'a FormulaCellRef>,
    ) -> Result<Vec<SheetCellChange>, AppError> {
        let mut changes = Vec::new();

        for target in targets {
            let Some(sheet) = file_data.sheets.get_mut(target.sheet_index) else {
                continue;
            };
            let Some(cell) = sheet
                .rows
                .get_mut(target.row)
                .and_then(|row_data| row_data.get_mut(target.col))
            else {
                continue;
            };
            if !matches!(cell, CellValue::Formula { .. }) {
                continue;
            }

            match self.workbook.evaluate_cell(
                &sheet.name,
                to_formula_index(target.row),
                to_formula_index(target.col),
            ) {
                Ok(value) => {
                    let (cached_value, error) = literal_to_cell(value);
                    *cell = cell.with_formula_result(cached_value, error);
                }
                Err(error) => {
                    *cell = cell.with_formula_result(CellValue::Null, Some(error.to_string()));
                }
            }

            changes.push(SheetCellChange {
                sheet_index: target.sheet_index,
                row: target.row,
                col: target.col,
                value: cell.clone(),
            });
        }

        Ok(changes)
    }
}

fn register_workbook_cells(
    workbook: &mut Workbook,
    file_data: &mut FileData,
) -> Result<FormulaRegistrationResult, AppError> {
    let mut result = FormulaRegistrationResult::default();

    for (sheet_index, sheet) in file_data.sheets.iter_mut().enumerate() {
        for (row_idx, row) in sheet.rows.iter_mut().enumerate() {
            for (col_idx, cell) in row.iter_mut().enumerate() {
                let cell_result =
                    set_workbook_cell(workbook, &sheet.name, sheet_index, row_idx, col_idx, cell)?;
                result
                    .registered_formulas
                    .extend(cell_result.registered_formulas);
                result.invalid_formulas.extend(cell_result.invalid_formulas);
            }
        }
    }

    Ok(result)
}

fn set_workbook_cell(
    workbook: &mut Workbook,
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
            match validate_formula(formula).and_then(|_| {
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
                        .set_value(sheet_name, row, col, LiteralValue::Empty)
                        .map_err(|error| AppError::Internal(error.to_string()))?;
                    let value = cell.with_formula_result(CellValue::Null, Some(error));
                    result.invalid_formulas.push(SheetCellChange {
                        sheet_index,
                        row: row_idx,
                        col: col_idx,
                        value,
                    });
                }
            }
        }
        _ => workbook
            .set_value(sheet_name, row, col, cell_to_literal(cell))
            .map_err(|error| AppError::Internal(error.to_string()))?,
    }

    Ok(result)
}

fn validate_formula(formula: &str) -> Result<(), String> {
    formualizer_parse::parser::parse(formula)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn apply_cell_changes(file_data: &mut FileData, changes: &[SheetCellChange]) {
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

fn build_dependency_index(
    file_data: &FileData,
    registered_formulas: &HashSet<FormulaCellRef>,
) -> FormulaDependencyIndex {
    let mut index = FormulaDependencyIndex::default();
    let sheet_indexes: HashMap<&str, usize> = file_data
        .sheets
        .iter()
        .enumerate()
        .map(|(sheet_index, sheet)| (sheet.name.as_str(), sheet_index))
        .collect();

    for (sheet_index, sheet) in file_data.sheets.iter().enumerate() {
        for (row_idx, row) in sheet.rows.iter().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
                let CellValue::Formula { formula, .. } = cell else {
                    continue;
                };
                let formula_ref = FormulaCellRef {
                    sheet_index,
                    row: row_idx,
                    col: col_idx,
                };
                if !registered_formulas.contains(&formula_ref) {
                    continue;
                }

                index.formulas.insert(formula_ref.clone());

                let dependencies =
                    match collect_formula_dependencies(formula, sheet_index, &sheet_indexes) {
                        DependencyCollection::Precise(dependencies) => dependencies,
                        DependencyCollection::Volatile => {
                            index.diagnostics.volatile_formula_count += 1;
                            index.always_recalculate.insert(formula_ref);
                            continue;
                        }
                        DependencyCollection::LargeRange => {
                            index.diagnostics.large_range_dependency_count += 1;
                            index.always_recalculate.insert(formula_ref);
                            continue;
                        }
                        DependencyCollection::Unsupported => {
                            index.diagnostics.unsupported_dependency_count += 1;
                            index.always_recalculate.insert(formula_ref);
                            continue;
                        }
                    };

                for dependency in dependencies {
                    index
                        .dependents_by_source
                        .entry(dependency)
                        .or_default()
                        .insert(formula_ref.clone());
                }
            }
        }
    }

    index
}

fn count_unregistered_formula_cells(
    file_data: &FileData,
    registered_formulas: &HashSet<FormulaCellRef>,
) -> usize {
    file_data
        .sheets
        .iter()
        .enumerate()
        .map(|(sheet_index, sheet)| {
            sheet
                .rows
                .iter()
                .enumerate()
                .map(|(row, row_data)| {
                    row_data
                        .iter()
                        .enumerate()
                        .filter(|(col, cell)| {
                            matches!(cell, CellValue::Formula { .. })
                                && !registered_formulas.contains(&FormulaCellRef {
                                    sheet_index,
                                    row,
                                    col: *col,
                                })
                        })
                        .count()
                })
                .sum::<usize>()
        })
        .sum()
}

enum DependencyCollection {
    Precise(HashSet<FormulaCellRef>),
    Volatile,
    LargeRange,
    Unsupported,
}

fn collect_formula_dependencies(
    formula: &str,
    current_sheet_index: usize,
    sheet_indexes: &HashMap<&str, usize>,
) -> DependencyCollection {
    let Ok(ast) =
        formualizer_parse::parse_with_volatility_classifier(formula, is_volatile_function)
    else {
        return DependencyCollection::Unsupported;
    };
    if ast.contains_volatile() {
        return DependencyCollection::Volatile;
    }

    let mut dependencies = HashSet::new();
    for reference in ast.collect_references(&CollectPolicy::default()) {
        match reference {
            ReferenceType::Cell {
                sheet, row, col, ..
            } => {
                dependencies.insert(FormulaCellRef {
                    sheet_index: match resolve_reference_sheet(
                        sheet.as_deref(),
                        current_sheet_index,
                        sheet_indexes,
                    ) {
                        Some(sheet_index) => sheet_index,
                        None => return DependencyCollection::Unsupported,
                    },
                    row: match to_zero_based(row) {
                        Some(row) => row,
                        None => return DependencyCollection::Unsupported,
                    },
                    col: match to_zero_based(col) {
                        Some(col) => col,
                        None => return DependencyCollection::Unsupported,
                    },
                });
            }
            ReferenceType::Range {
                sheet,
                start_row: Some(start_row),
                start_col: Some(start_col),
                end_row: Some(end_row),
                end_col: Some(end_col),
                ..
            } => {
                let sheet_index = match resolve_reference_sheet(
                    sheet.as_deref(),
                    current_sheet_index,
                    sheet_indexes,
                ) {
                    Some(sheet_index) => sheet_index,
                    None => return DependencyCollection::Unsupported,
                };
                if start_row > end_row || start_col > end_col {
                    return DependencyCollection::Unsupported;
                }
                let height = end_row.saturating_sub(start_row) + 1;
                let width = end_col.saturating_sub(start_col) + 1;
                if height.saturating_mul(width) > MAX_RANGE_DEPENDENCY_CELLS {
                    return DependencyCollection::LargeRange;
                }
                for row in start_row..=end_row {
                    for col in start_col..=end_col {
                        dependencies.insert(FormulaCellRef {
                            sheet_index,
                            row: match to_zero_based(row) {
                                Some(row) => row,
                                None => return DependencyCollection::Unsupported,
                            },
                            col: match to_zero_based(col) {
                                Some(col) => col,
                                None => return DependencyCollection::Unsupported,
                            },
                        });
                    }
                }
            }
            _ => return DependencyCollection::Unsupported,
        }
    }

    DependencyCollection::Precise(dependencies)
}

fn resolve_reference_sheet(
    sheet_name: Option<&str>,
    current_sheet_index: usize,
    sheet_indexes: &HashMap<&str, usize>,
) -> Option<usize> {
    sheet_name
        .map(|name| sheet_indexes.get(name).copied())
        .unwrap_or(Some(current_sheet_index))
}

fn to_zero_based(index: u32) -> Option<usize> {
    usize::try_from(index.checked_sub(1)?).ok()
}

fn is_volatile_function(name: &str) -> bool {
    matches!(
        name.to_ascii_uppercase().as_str(),
        "NOW" | "TODAY" | "RAND" | "RANDBETWEEN" | "OFFSET" | "INDIRECT" | "INFO" | "CELL"
    )
}

fn to_formula_index(index: usize) -> u32 {
    index.saturating_add(1) as u32
}

fn cell_to_literal(cell: &CellValue) -> LiteralValue {
    match cell {
        CellValue::Null => LiteralValue::Empty,
        CellValue::String(value) => LiteralValue::Text(value.clone()),
        CellValue::Number(value) => {
            if let Some(int) = value.as_i64() {
                LiteralValue::Int(int)
            } else if let Some(float) = value.as_f64() {
                LiteralValue::Number(float)
            } else {
                LiteralValue::Text(value.to_string())
            }
        }
        CellValue::Boolean(value) => LiteralValue::Boolean(*value),
        CellValue::Formula { cached_value, .. } => cell_to_literal(cached_value),
    }
}

fn literal_to_cell(value: LiteralValue) -> (CellValue, Option<String>) {
    match value {
        LiteralValue::Empty | LiteralValue::Pending => (CellValue::Null, None),
        LiteralValue::Int(value) => (CellValue::Number(Value::from(value)), None),
        LiteralValue::Number(value) => (CellValue::Number(Value::from(value)), None),
        LiteralValue::Text(value) => (CellValue::String(value), None),
        LiteralValue::Boolean(value) => (CellValue::Boolean(value), None),
        LiteralValue::Error(error) => (CellValue::Null, Some(error.kind.to_string())),
        LiteralValue::Array(values) => values
            .first()
            .and_then(|row| row.first())
            .cloned()
            .map(literal_to_cell)
            .unwrap_or((CellValue::Null, None)),
        LiteralValue::Date(value) => (CellValue::String(value.to_string()), None),
        LiteralValue::DateTime(value) => (CellValue::String(value.to_string()), None),
        LiteralValue::Time(value) => (CellValue::String(value.to_string()), None),
        LiteralValue::Duration(value) => (CellValue::String(value.to_string()), None),
    }
}

#[cfg(test)]
mod tests {
    use crate::types::SheetData;

    use super::*;

    #[test]
    fn recalculates_basic_formulas() {
        let mut file_data = FileData {
            path: String::new(),
            file_name: "formula.xlsx".to_string(),
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows: vec![
                    vec![
                        CellValue::Number(Value::from(2)),
                        CellValue::Number(Value::from(3)),
                    ],
                    vec![
                        CellValue::formula("=A1+B1", CellValue::Null),
                        CellValue::formula("=SUM(A1:B1)", CellValue::Null),
                    ],
                ],
                ..Default::default()
            }],
        };

        FormulaRuntime::new(&mut file_data).expect("formula recalc");

        assert_eq!(file_data.sheets[0].rows[1][0].to_display_string(), "5.0");
        assert_eq!(file_data.sheets[0].rows[1][1].to_display_string(), "5.0");
    }

    #[test]
    fn rebuild_keeps_valid_formulas_working_when_one_formula_is_invalid() {
        let mut file_data = FileData {
            path: String::new(),
            file_name: "formula.xlsx".to_string(),
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows: vec![vec![
                    CellValue::Number(Value::from(1)),
                    CellValue::formula("=SUM(", CellValue::Null),
                    CellValue::formula("=A1+1", CellValue::Null),
                ]],
                ..Default::default()
            }],
        };

        let mut runtime = FormulaRuntime::new(&mut file_data).expect("formula runtime");

        assert!(matches!(
            &file_data.sheets[0].rows[0][1],
            CellValue::Formula { error: Some(_), .. }
        ));
        assert_eq!(file_data.sheets[0].rows[0][2].to_display_string(), "2.0");

        file_data.sheets[0].rows[0][0] = CellValue::Number(Value::from(10));
        runtime
            .sync_cell_and_recalculate(&mut file_data, 0, 0, 0)
            .expect("incremental recalc");

        assert_eq!(file_data.sheets[0].rows[0][2].to_display_string(), "11.0");
    }

    #[test]
    fn invalid_formula_edit_returns_cell_error_and_keeps_other_formulas_live() {
        let mut file_data = FileData {
            path: String::new(),
            file_name: "formula.xlsx".to_string(),
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows: vec![vec![
                    CellValue::Number(Value::from(1)),
                    CellValue::formula("=A1+1", CellValue::Null),
                    CellValue::formula("=A1+2", CellValue::Null),
                ]],
                ..Default::default()
            }],
        };

        let mut runtime = FormulaRuntime::new(&mut file_data).expect("formula runtime");
        assert_eq!(file_data.sheets[0].rows[0][1].to_display_string(), "2.0");
        assert_eq!(file_data.sheets[0].rows[0][2].to_display_string(), "3.0");

        file_data.sheets[0].rows[0][1] = CellValue::formula("=SUM(", CellValue::Null);
        let changes = runtime
            .sync_cell_and_recalculate(&mut file_data, 0, 0, 1)
            .expect("invalid formula is isolated");

        assert!(changes.iter().any(|change| {
            change.sheet_index == 0
                && change.row == 0
                && change.col == 1
                && matches!(&change.value, CellValue::Formula { error: Some(_), .. })
        }));
        assert!(matches!(
            &file_data.sheets[0].rows[0][1],
            CellValue::Formula { error: Some(_), .. }
        ));
        assert_eq!(runtime.diagnostics().invalid_formula_count, 1);

        file_data.sheets[0].rows[0][0] = CellValue::Number(Value::from(10));
        runtime
            .sync_cell_and_recalculate(&mut file_data, 0, 0, 0)
            .expect("incremental recalc");

        assert_eq!(file_data.sheets[0].rows[0][2].to_display_string(), "12.0");
        assert!(matches!(
            &file_data.sheets[0].rows[0][1],
            CellValue::Formula { error: Some(_), .. }
        ));
        assert_eq!(runtime.diagnostics().invalid_formula_count, 1);
    }

    #[test]
    fn diagnostics_report_formula_dependency_fallbacks() {
        let mut file_data = FileData {
            path: String::new(),
            file_name: "diagnostics.xlsx".to_string(),
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows: vec![vec![
                    CellValue::formula("=SUM(", CellValue::Null),
                    CellValue::formula("=NOW()", CellValue::Null),
                    CellValue::formula("=SUM(A1:A10001)", CellValue::Null),
                    CellValue::formula("=SUM(A:A)", CellValue::Null),
                ]],
                ..Default::default()
            }],
        };

        let runtime = FormulaRuntime::new(&mut file_data).expect("formula runtime");
        let diagnostics = runtime.diagnostics();

        assert_eq!(diagnostics.invalid_formula_count, 1);
        assert_eq!(diagnostics.volatile_formula_count, 1);
        assert_eq!(diagnostics.large_range_dependency_count, 1);
        assert_eq!(diagnostics.unsupported_dependency_count, 1);
    }

    #[test]
    fn batch_formula_edit_returns_cell_error_and_keeps_other_formulas_live() {
        let mut file_data = FileData {
            path: String::new(),
            file_name: "formula.xlsx".to_string(),
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows: vec![vec![
                    CellValue::Number(Value::from(1)),
                    CellValue::formula("=A1+1", CellValue::Null),
                    CellValue::formula("=A1+2", CellValue::Null),
                    CellValue::Number(Value::from(0)),
                ]],
                ..Default::default()
            }],
        };

        let mut runtime = FormulaRuntime::new(&mut file_data).expect("formula runtime");
        file_data.sheets[0].rows[0][1] = CellValue::formula("=SUM(", CellValue::Null);
        file_data.sheets[0].rows[0][3] = CellValue::formula("=A1+3", CellValue::Null);

        let changes = runtime
            .sync_cells_and_recalculate(
                &mut file_data,
                [
                    FormulaCellRef {
                        sheet_index: 0,
                        row: 0,
                        col: 1,
                    },
                    FormulaCellRef {
                        sheet_index: 0,
                        row: 0,
                        col: 3,
                    },
                ],
            )
            .expect("batch recalc isolates invalid formulas");

        assert!(changes.iter().any(|change| {
            change.sheet_index == 0
                && change.row == 0
                && change.col == 1
                && matches!(&change.value, CellValue::Formula { error: Some(_), .. })
        }));
        assert_eq!(file_data.sheets[0].rows[0][2].to_display_string(), "3.0");
        assert_eq!(file_data.sheets[0].rows[0][3].to_display_string(), "4.0");

        file_data.sheets[0].rows[0][0] = CellValue::Number(Value::from(10));
        runtime
            .sync_cell_and_recalculate(&mut file_data, 0, 0, 0)
            .expect("incremental recalc remains live");

        assert_eq!(file_data.sheets[0].rows[0][2].to_display_string(), "12.0");
        assert_eq!(file_data.sheets[0].rows[0][3].to_display_string(), "13.0");
        assert!(matches!(
            &file_data.sheets[0].rows[0][1],
            CellValue::Formula { error: Some(_), .. }
        ));
    }

    #[test]
    fn incrementally_recalculates_after_value_change() {
        let mut file_data = FileData {
            path: String::new(),
            file_name: "formula.xlsx".to_string(),
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows: vec![vec![
                    CellValue::Number(Value::from(2)),
                    CellValue::formula("=A1+1", CellValue::Null),
                ]],
                ..Default::default()
            }],
        };

        let mut runtime = FormulaRuntime::new(&mut file_data).expect("formula runtime");
        assert_eq!(file_data.sheets[0].rows[0][1].to_display_string(), "3.0");

        file_data.sheets[0].rows[0][0] = CellValue::Number(Value::from(10));
        runtime
            .sync_cell_and_recalculate(&mut file_data, 0, 0, 0)
            .expect("incremental recalc");

        assert_eq!(file_data.sheets[0].rows[0][1].to_display_string(), "11.0");
    }

    #[test]
    fn incrementally_recalculates_after_formula_change() {
        let mut file_data = FileData {
            path: String::new(),
            file_name: "formula.xlsx".to_string(),
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows: vec![vec![
                    CellValue::Number(Value::from(10)),
                    CellValue::formula("=A1+1", CellValue::Null),
                ]],
                ..Default::default()
            }],
        };

        let mut runtime = FormulaRuntime::new(&mut file_data).expect("formula runtime");
        assert_eq!(file_data.sheets[0].rows[0][1].to_display_string(), "11.0");

        file_data.sheets[0].rows[0][1] = CellValue::formula("=A1*2", CellValue::Null);
        runtime
            .sync_cell_and_recalculate(&mut file_data, 0, 0, 1)
            .expect("incremental recalc");

        assert_eq!(file_data.sheets[0].rows[0][1].to_display_string(), "20.0");
    }

    #[test]
    fn incrementally_recalculates_when_value_becomes_formula() {
        let mut file_data = FileData {
            path: String::new(),
            file_name: "formula.xlsx".to_string(),
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows: vec![vec![
                    CellValue::Number(Value::from(5)),
                    CellValue::Number(Value::from(0)),
                ]],
                ..Default::default()
            }],
        };

        let mut runtime = FormulaRuntime::new(&mut file_data).expect("formula runtime");

        file_data.sheets[0].rows[0][1] = CellValue::formula("=A1*4", CellValue::Null);
        runtime
            .sync_cell_and_recalculate(&mut file_data, 0, 0, 1)
            .expect("incremental recalc");

        assert_eq!(file_data.sheets[0].rows[0][1].to_display_string(), "20.0");
    }

    #[test]
    fn incrementally_recalculates_dependents_when_formula_becomes_value() {
        let mut file_data = FileData {
            path: String::new(),
            file_name: "formula.xlsx".to_string(),
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows: vec![vec![
                    CellValue::formula("=5", CellValue::Null),
                    CellValue::formula("=A1+1", CellValue::Null),
                ]],
                ..Default::default()
            }],
        };

        let mut runtime = FormulaRuntime::new(&mut file_data).expect("formula runtime");
        assert_eq!(file_data.sheets[0].rows[0][1].to_display_string(), "6.0");

        file_data.sheets[0].rows[0][0] = CellValue::Number(Value::from(10));
        runtime
            .sync_cell_and_recalculate(&mut file_data, 0, 0, 0)
            .expect("incremental recalc");

        assert_eq!(file_data.sheets[0].rows[0][1].to_display_string(), "11.0");
    }

    #[test]
    fn incrementally_recalculates_dependency_closure() {
        let mut file_data = FileData {
            path: String::new(),
            file_name: "formula.xlsx".to_string(),
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows: vec![vec![
                    CellValue::Number(Value::from(1)),
                    CellValue::formula("=A1+1", CellValue::Null),
                    CellValue::formula("=B1+1", CellValue::Null),
                ]],
                ..Default::default()
            }],
        };

        let mut runtime = FormulaRuntime::new(&mut file_data).expect("formula runtime");
        assert_eq!(file_data.sheets[0].rows[0][2].to_display_string(), "3.0");

        file_data.sheets[0].rows[0][0] = CellValue::Number(Value::from(5));
        runtime
            .sync_cell_and_recalculate(&mut file_data, 0, 0, 0)
            .expect("incremental recalc");

        assert_eq!(file_data.sheets[0].rows[0][1].to_display_string(), "6.0");
        assert_eq!(file_data.sheets[0].rows[0][2].to_display_string(), "7.0");
    }

    #[test]
    fn incrementally_recalculates_cross_sheet_dependencies() {
        let mut file_data = FileData {
            path: String::new(),
            file_name: "formula.xlsx".to_string(),
            sheets: vec![
                SheetData {
                    name: "Inputs".to_string(),
                    rows: vec![vec![CellValue::Number(Value::from(4))]],
                    ..Default::default()
                },
                SheetData {
                    name: "Summary".to_string(),
                    rows: vec![vec![CellValue::formula("=Inputs!A1*3", CellValue::Null)]],
                    ..Default::default()
                },
            ],
        };

        let mut runtime = FormulaRuntime::new(&mut file_data).expect("formula runtime");
        assert_eq!(file_data.sheets[1].rows[0][0].to_display_string(), "12.0");

        file_data.sheets[0].rows[0][0] = CellValue::Number(Value::from(7));
        runtime
            .sync_cell_and_recalculate(&mut file_data, 0, 0, 0)
            .expect("incremental recalc");

        assert_eq!(file_data.sheets[1].rows[0][0].to_display_string(), "21.0");
    }

    #[test]
    fn unchanged_formula_cache_is_not_refreshed_for_unrelated_edit() {
        let mut file_data = FileData {
            path: String::new(),
            file_name: "formula.xlsx".to_string(),
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows: vec![vec![
                    CellValue::Number(Value::from(1)),
                    CellValue::formula("=A1+1", CellValue::Null),
                    CellValue::Number(Value::from(5)),
                    CellValue::Formula {
                        formula: "=C1+1".to_string(),
                        cached_value: Box::new(CellValue::String("stale".to_string())),
                        error: None,
                    },
                ]],
                ..Default::default()
            }],
        };

        let mut runtime = FormulaRuntime::new(&mut file_data).expect("formula runtime");
        file_data.sheets[0].rows[0][3] = CellValue::Formula {
            formula: "=C1+1".to_string(),
            cached_value: Box::new(CellValue::String("stale".to_string())),
            error: None,
        };

        file_data.sheets[0].rows[0][0] = CellValue::Number(Value::from(2));
        runtime
            .sync_cell_and_recalculate(&mut file_data, 0, 0, 0)
            .expect("incremental recalc");

        assert_eq!(file_data.sheets[0].rows[0][1].to_display_string(), "3.0");
        assert_eq!(file_data.sheets[0].rows[0][3].to_display_string(), "stale");
    }
}
