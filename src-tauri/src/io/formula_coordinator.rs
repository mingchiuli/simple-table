use crate::formula::ast::FormulaAstService;
use crate::formula::cell_ref::FormulaCellRef;
use crate::formula::engine::FormulaRuntime;
use crate::io::workbook_state::StructurePatchDiagnostics;
use crate::ops::AppliedOperation;
use crate::types::{CellValue, FileData, FormulaDiagnostics, FormulaStatus, SheetCellChange};

pub(crate) struct FormulaCoordinator {
    runtime: FormulaRuntime,
    ast_service: FormulaAstService,
    status: FormulaStatus,
    pending_structure_diagnostics: StructurePatchDiagnostics,
}

impl FormulaCoordinator {
    pub(crate) fn new(projection: &mut FileData) -> Self {
        let mut ast_service = FormulaAstService::new();
        match FormulaRuntime::new(projection, &mut ast_service) {
            Ok(runtime) => {
                let status = FormulaStatus::ready(runtime.diagnostics());
                Self {
                    runtime,
                    ast_service,
                    status,
                    pending_structure_diagnostics: StructurePatchDiagnostics::default(),
                }
            }
            Err(error) => {
                eprintln!("Formula runtime initialization failed: {error}");
                Self {
                    runtime: FormulaRuntime::empty(),
                    ast_service,
                    status: FormulaStatus::degraded(
                        error.to_string(),
                        FormulaDiagnostics::default(),
                    ),
                    pending_structure_diagnostics: StructurePatchDiagnostics::default(),
                }
            }
        }
    }

    pub(crate) fn status(&self) -> FormulaStatus {
        self.status.clone()
    }

    pub(crate) fn ast_service_mut(&mut self) -> &mut FormulaAstService {
        &mut self.ast_service
    }

    pub(crate) fn set_pending_structure_diagnostics(
        &mut self,
        diagnostics: StructurePatchDiagnostics,
    ) {
        self.pending_structure_diagnostics = diagnostics;
    }

    pub(crate) fn mark_degraded(&mut self, reason: String) {
        self.status = FormulaStatus::degraded(reason, FormulaDiagnostics::default());
    }

    pub(crate) fn structure_formula_limitations(&self) -> Vec<String> {
        let diagnostics = match &self.status {
            FormulaStatus::Ready { diagnostics } | FormulaStatus::Degraded { diagnostics, .. } => {
                diagnostics
            }
        };
        let mut limitations = Vec::new();
        if diagnostics.invalid_formula_count > 0 {
            limitations.push("unparseable formulas".to_string());
        }
        if diagnostics.unsupported_dependency_count > 0 {
            limitations.push("unsupported formula references".to_string());
        }
        limitations
    }

    pub(crate) fn structure_memento_sheet_indexes(
        &self,
        projection: &FileData,
        operation: &AppliedOperation,
    ) -> Vec<usize> {
        if !operation.impact().is_structure_change() {
            return Vec::new();
        }
        let mut sheet_indexes = std::collections::BTreeSet::new();
        for cell in self.formula_cell_positions(projection) {
            sheet_indexes.insert(cell.sheet_index);
        }
        sheet_indexes.into_iter().collect()
    }

    pub(crate) fn impacted_cells_for_memento(
        &self,
        changed_cells: impl IntoIterator<Item = FormulaCellRef>,
        projection: &FileData,
    ) -> Vec<FormulaCellRef> {
        let changed_cells: Vec<FormulaCellRef> = changed_cells.into_iter().collect();
        match &self.status {
            FormulaStatus::Ready { .. } => self
                .runtime
                .impacted_formula_cells_for(changed_cells.iter().copied()),
            FormulaStatus::Degraded { .. } => self.formula_cell_positions(projection),
        }
    }

    pub(crate) fn recalculate_after_operation(
        &mut self,
        operation: &AppliedOperation,
        projection: &mut FileData,
    ) -> Vec<SheetCellChange> {
        match operation {
            AppliedOperation::SetCell {
                sheet_index,
                row,
                col,
                new_value,
                ..
            } => {
                let result = self.runtime.sync_cell_and_recalculate(
                    projection,
                    &mut self.ast_service,
                    *sheet_index,
                    *row,
                    *col,
                );

                match result {
                    Ok(changes) => {
                        self.status = FormulaStatus::ready(self.runtime.diagnostics());
                        changes
                    }
                    Err(error) => {
                        eprintln!("Formula recalculation failed: {error}");
                        let mut changes = formula_error_change(
                            projection,
                            *sheet_index,
                            *row,
                            *col,
                            new_value,
                            error.to_string(),
                        );
                        append_unique_changes(&mut changes, self.rebuild(projection));
                        changes
                    }
                }
            }
            AppliedOperation::SetCells { changes } => {
                let changed_cell_refs: Vec<FormulaCellRef> = changes
                    .iter()
                    .map(|change| FormulaCellRef {
                        sheet_index: change.sheet_index,
                        row: change.row,
                        col: change.col,
                    })
                    .collect();
                match self.runtime.sync_cells_and_recalculate(
                    projection,
                    &mut self.ast_service,
                    changed_cell_refs,
                ) {
                    Ok(changes) => {
                        self.status = FormulaStatus::ready(self.runtime.diagnostics());
                        changes
                    }
                    Err(error) => {
                        eprintln!("Formula recalculation failed: {error}");
                        let error = error.to_string();
                        let mut formula_errors = Vec::new();
                        for change in changes {
                            formula_errors.extend(formula_error_change(
                                projection,
                                change.sheet_index,
                                change.row,
                                change.col,
                                &change.new_value,
                                error.clone(),
                            ));
                        }
                        append_unique_changes(&mut formula_errors, self.rebuild(projection));
                        formula_errors
                    }
                }
            }
            AppliedOperation::SetColumnWidth { .. } | AppliedOperation::SetRowHeight { .. } => {
                Vec::new()
            }
            _ => match self
                .runtime
                .rebuild_and_recalculate_with_diagnostics(projection, &mut self.ast_service)
            {
                Ok(result) => {
                    let mut diagnostics = result.diagnostics;
                    self.merge_structure_diagnostics(&mut diagnostics);
                    self.status = FormulaStatus::ready(diagnostics);
                    result.changes
                }
                Err(error) => {
                    eprintln!("Formula recalculation failed: {error}");
                    self.formula_error_changes_for_all_formulas(projection, error.to_string())
                }
            },
        }
    }

    pub(crate) fn rebuild(&mut self, projection: &mut FileData) -> Vec<SheetCellChange> {
        match self
            .runtime
            .rebuild_preserving_cached_results(projection, &mut self.ast_service)
        {
            Ok(result) => {
                let mut diagnostics = result.diagnostics;
                self.merge_structure_diagnostics(&mut diagnostics);
                self.status = FormulaStatus::ready(diagnostics);
                result.changes
            }
            Err(error) => {
                eprintln!("Formula runtime rebuild failed: {error}");
                self.runtime = FormulaRuntime::empty();
                self.status =
                    FormulaStatus::degraded(error.to_string(), FormulaDiagnostics::default());
                Vec::new()
            }
        }
    }

    fn formula_cell_positions(&self, projection: &FileData) -> Vec<FormulaCellRef> {
        let mut positions = self.runtime.all_formula_cells();
        let mut seen: std::collections::HashSet<_> = positions.iter().copied().collect();
        for (sheet_index, sheet) in projection.sheets.iter().enumerate() {
            for (row, row_data) in sheet.rows.iter().enumerate() {
                for (col, cell) in row_data.iter().enumerate() {
                    if matches!(cell, CellValue::Formula { .. }) {
                        let cell_ref = FormulaCellRef {
                            sheet_index,
                            row,
                            col,
                        };
                        if seen.insert(cell_ref) {
                            positions.push(cell_ref);
                        }
                    }
                }
            }
        }
        positions
    }

    fn formula_error_changes_for_all_formulas(
        &mut self,
        projection: &mut FileData,
        error: String,
    ) -> Vec<SheetCellChange> {
        let mut changes = Vec::new();
        for (sheet_index, sheet) in projection.sheets.iter_mut().enumerate() {
            for (row, row_data) in sheet.rows.iter_mut().enumerate() {
                for (col, cell) in row_data.iter_mut().enumerate() {
                    if !matches!(cell, CellValue::Formula { .. }) {
                        continue;
                    }
                    *cell = cell.with_formula_result(CellValue::Null, Some(error.clone()));
                    changes.push(SheetCellChange::new(sheet_index, row, col, cell.clone()));
                }
            }
        }
        self.runtime = FormulaRuntime::empty();
        self.status = FormulaStatus::degraded(error, FormulaDiagnostics::default());
        changes
    }

    fn merge_structure_diagnostics(&mut self, diagnostics: &mut FormulaDiagnostics) {
        diagnostics.skipped_reference_rewrite_count += self
            .pending_structure_diagnostics
            .skipped_formula_reference_rewrites;
        self.pending_structure_diagnostics = StructurePatchDiagnostics::default();
    }
}

fn formula_error_change(
    projection: &mut FileData,
    sheet_index: usize,
    row: usize,
    col: usize,
    value: &CellValue,
    error: String,
) -> Vec<SheetCellChange> {
    if !matches!(value, CellValue::Formula { .. }) {
        return Vec::new();
    }

    let Some(cell) = projection
        .sheets
        .get_mut(sheet_index)
        .and_then(|sheet| sheet.rows.get_mut(row))
        .and_then(|row_data| row_data.get_mut(col))
    else {
        return Vec::new();
    };

    *cell = cell.with_formula_result(CellValue::Null, Some(error));
    vec![SheetCellChange::new(sheet_index, row, col, cell.clone())]
}

fn append_unique_changes(target: &mut Vec<SheetCellChange>, changes: Vec<SheetCellChange>) {
    for change in changes {
        if let Some(existing) = target.iter_mut().find(|existing| {
            existing.sheet_index == change.sheet_index
                && existing.row == change.row
                && existing.col == change.col
        }) {
            *existing = change;
        } else {
            target.push(change);
        }
    }
}
