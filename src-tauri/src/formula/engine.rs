use std::collections::{HashSet, VecDeque};

use formualizer_workbook::{Workbook, WorkbookMode};

use crate::error::AppError;
use crate::formula::ast::FormulaAstService;
use crate::formula::cell_ref::FormulaCellRef;
use crate::formula::index::{
    FormulaDependencyIndex, build_dependency_index, count_unregistered_formula_cells,
};
use crate::formula::registry::{apply_cell_changes, register_workbook_cells, set_workbook_cell};
use crate::formula::value_codec::{literal_to_cell, to_formula_index};
use crate::types::{CellValue, FileData, FormulaDiagnostics, SheetCellChange};

pub struct FormulaRuntime {
    workbook: Workbook,
    ast_service: FormulaAstService,
    dependency_index: FormulaDependencyIndex,
    registered_formulas: HashSet<FormulaCellRef>,
}

pub struct FormulaRebuildResult {
    pub changes: Vec<SheetCellChange>,
    pub diagnostics: FormulaDiagnostics,
}

#[derive(Clone, Copy)]
enum FormulaRebuildPolicy {
    PreserveCachedResults,
    RecalculateRegisteredFormulas,
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
            ast_service: FormulaAstService::new(),
            dependency_index: FormulaDependencyIndex::default(),
            registered_formulas: HashSet::new(),
        }
    }

    pub fn rebuild(&mut self, file_data: &mut FileData) -> Result<Vec<SheetCellChange>, AppError> {
        Ok(self.rebuild_preserving_cached_results(file_data)?.changes)
    }

    pub fn rebuild_preserving_cached_results(
        &mut self,
        file_data: &mut FileData,
    ) -> Result<FormulaRebuildResult, AppError> {
        self.rebuild_with_policy(file_data, FormulaRebuildPolicy::PreserveCachedResults)
    }

    pub fn rebuild_and_recalculate_with_diagnostics(
        &mut self,
        file_data: &mut FileData,
    ) -> Result<FormulaRebuildResult, AppError> {
        self.rebuild_with_policy(
            file_data,
            FormulaRebuildPolicy::RecalculateRegisteredFormulas,
        )
    }

    fn rebuild_with_policy(
        &mut self,
        file_data: &mut FileData,
        policy: FormulaRebuildPolicy,
    ) -> Result<FormulaRebuildResult, AppError> {
        let mut workbook = Workbook::new_with_mode(WorkbookMode::Ephemeral);
        for sheet in &file_data.sheets {
            workbook
                .add_sheet(&sheet.name)
                .map_err(|error| AppError::Internal(error.to_string()))?;
        }

        let registration_result =
            register_workbook_cells(&mut workbook, &mut self.ast_service, file_data)?;
        let mut changes = Vec::new();

        let sheet_names: Vec<String> = file_data
            .sheets
            .iter()
            .map(|sheet| sheet.name.clone())
            .collect();
        workbook
            .prepare_graph_for_sheets(sheet_names.iter().map(String::as_str))
            .map_err(|error| AppError::Internal(error.to_string()))?;

        self.workbook = workbook;
        self.registered_formulas = registration_result.registered_formulas;
        self.dependency_index =
            build_dependency_index(file_data, &self.registered_formulas, &mut self.ast_service);
        self.refresh_formula_diagnostics(file_data);
        if matches!(policy, FormulaRebuildPolicy::RecalculateRegisteredFormulas) {
            changes.extend(self.recalculate_all_formula_cells(file_data)?);
        }
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

        let registration_result = set_workbook_cell(
            &mut self.workbook,
            &mut self.ast_service,
            &sheet.name,
            sheet_index,
            row,
            col,
            cell,
        )?;
        let mut changes = registration_result.invalid_formulas;
        if registration_result.registered_formulas.contains(&cell_ref) {
            self.registered_formulas.insert(cell_ref);
        } else {
            self.registered_formulas.remove(&cell_ref);
        }
        apply_cell_changes(file_data, &changes);

        if was_formula || is_formula {
            self.dependency_index.update_formula_dependencies(
                file_data,
                [cell_ref],
                &self.registered_formulas,
                &mut self.ast_service,
            );
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
        let mut dependency_updates = Vec::new();

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
                &mut self.ast_service,
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
            if was_formula || is_formula {
                dependency_updates.push(*cell_ref);
            }
        }

        apply_cell_changes(file_data, &changes);

        if !dependency_updates.is_empty() {
            self.dependency_index.update_formula_dependencies(
                file_data,
                dependency_updates,
                &self.registered_formulas,
                &mut self.ast_service,
            );
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
            if let Some(dependents) = self.dependency_index.dependents_by_source.get(&source) {
                for dependent in dependents {
                    if impacted.insert(*dependent) {
                        queue.push_back(*dependent);
                    }
                }
            }

            for dependent in self
                .dependency_index
                .range_dependents
                .dependents_for(source)
            {
                if impacted.insert(dependent) {
                    queue.push_back(dependent);
                }
            }

            for dependent in self
                .dependency_index
                .large_range_dependents
                .dependents_for(source)
            {
                if impacted.insert(dependent) {
                    queue.push_back(dependent);
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

            changes.push(SheetCellChange::new(
                target.sheet_index,
                target.row,
                target.col,
                cell.clone(),
            ));
        }

        Ok(changes)
    }
}

#[cfg(test)]
mod tests {
    use crate::types::SheetData;
    use serde_json::Value;

    use super::*;

    #[test]
    fn rebuild_preserves_cached_formula_results_until_an_edit_impacts_them() {
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
                        CellValue::formula("=A1+B1", CellValue::String("cached".to_string())),
                        CellValue::formula("=SUM(A1:B1)", CellValue::String("cached".to_string())),
                    ],
                ],
                ..Default::default()
            }],
        };

        let mut runtime = FormulaRuntime::new(&mut file_data).expect("formula runtime");

        assert_eq!(file_data.sheets[0].rows[1][0].to_display_string(), "cached");
        assert_eq!(file_data.sheets[0].rows[1][1].to_display_string(), "cached");

        file_data.sheets[0].rows[0][0] = CellValue::Number(Value::from(4));
        runtime
            .sync_cell_and_recalculate(&mut file_data, 0, 0, 0)
            .expect("incremental recalc");

        assert_eq!(file_data.sheets[0].rows[1][0].to_display_string(), "7.0");
        assert_eq!(file_data.sheets[0].rows[1][1].to_display_string(), "7.0");
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

        assert_eq!(runtime.diagnostics().invalid_formula_count, 1);
        assert_eq!(file_data.sheets[0].rows[0][2].to_display_string(), "");

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
    fn large_bounded_ranges_recalculate_only_when_the_source_is_inside_the_range() {
        let mut row = vec![CellValue::Null; 3];
        row[0] = CellValue::Number(Value::from(1));
        row[1] = CellValue::formula("=SUM(A1:A10001)", CellValue::Null);
        row[2] = CellValue::Number(Value::from(0));
        let mut file_data = FileData {
            path: String::new(),
            file_name: "range.xlsx".to_string(),
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows: vec![row],
                ..Default::default()
            }],
        };

        let mut runtime = FormulaRuntime::new(&mut file_data).expect("formula runtime");
        assert_eq!(runtime.diagnostics().large_range_dependency_count, 1);

        file_data.sheets[0].rows[0][0] = CellValue::Number(Value::from(5));
        runtime
            .sync_cell_and_recalculate(&mut file_data, 0, 0, 0)
            .expect("incremental range recalc");

        assert_eq!(file_data.sheets[0].rows[0][1].to_display_string(), "5.0");

        file_data.sheets[0].rows[0][2] = CellValue::Number(Value::from(10));
        let changes = runtime
            .sync_cell_and_recalculate(&mut file_data, 0, 0, 2)
            .expect("unrelated edit");

        assert!(
            !changes
                .iter()
                .any(|change| change.sheet_index == 0 && change.row == 0 && change.col == 1)
        );
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

        file_data.sheets[0].rows[0][1] = CellValue::formula("=A1*2", CellValue::Null);
        runtime
            .sync_cell_and_recalculate(&mut file_data, 0, 0, 1)
            .expect("incremental recalc");

        assert_eq!(file_data.sheets[0].rows[0][1].to_display_string(), "20.0");
    }

    #[test]
    fn formula_dependency_update_replaces_old_edges_and_diagnostics() {
        let mut file_data = FileData {
            path: String::new(),
            file_name: "formula.xlsx".to_string(),
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows: vec![vec![
                    CellValue::Number(Value::from(1)),
                    CellValue::formula("=SUM(A1:A10001)", CellValue::Null),
                    CellValue::Number(Value::from(10)),
                ]],
                ..Default::default()
            }],
        };

        let mut runtime = FormulaRuntime::new(&mut file_data).expect("formula runtime");
        assert_eq!(runtime.diagnostics().large_range_dependency_count, 1);

        file_data.sheets[0].rows[0][1] = CellValue::formula("=C1+1", CellValue::Null);
        runtime
            .sync_cell_and_recalculate(&mut file_data, 0, 0, 1)
            .expect("formula dependency update");

        assert_eq!(runtime.diagnostics().large_range_dependency_count, 0);
        assert_eq!(file_data.sheets[0].rows[0][1].to_display_string(), "11.0");

        file_data.sheets[0].rows[0][0] = CellValue::Number(Value::from(99));
        let changes = runtime
            .sync_cell_and_recalculate(&mut file_data, 0, 0, 0)
            .expect("old dependency should not recalc");
        assert!(
            !changes
                .iter()
                .any(|change| change.sheet_index == 0 && change.row == 0 && change.col == 1)
        );

        file_data.sheets[0].rows[0][2] = CellValue::Number(Value::from(20));
        runtime
            .sync_cell_and_recalculate(&mut file_data, 0, 0, 2)
            .expect("new dependency should recalc");
        assert_eq!(file_data.sheets[0].rows[0][1].to_display_string(), "21.0");
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

    #[test]
    fn range_formula_cache_is_not_refreshed_for_outside_edit() {
        let mut file_data = FileData {
            path: String::new(),
            file_name: "range.xlsx".to_string(),
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows: vec![vec![
                    CellValue::Number(Value::from(1)),
                    CellValue::Number(Value::from(2)),
                    CellValue::Formula {
                        formula: "=SUM(A1:B1)".to_string(),
                        cached_value: Box::new(CellValue::String("stale".to_string())),
                        error: None,
                    },
                    CellValue::Number(Value::from(5)),
                ]],
                ..Default::default()
            }],
        };

        let mut runtime = FormulaRuntime::new(&mut file_data).expect("formula runtime");
        file_data.sheets[0].rows[0][2] = CellValue::Formula {
            formula: "=SUM(A1:B1)".to_string(),
            cached_value: Box::new(CellValue::String("stale".to_string())),
            error: None,
        };

        file_data.sheets[0].rows[0][3] = CellValue::Number(Value::from(10));
        runtime
            .sync_cell_and_recalculate(&mut file_data, 0, 0, 3)
            .expect("incremental recalc");

        assert_eq!(file_data.sheets[0].rows[0][2].to_display_string(), "stale");
    }
}
