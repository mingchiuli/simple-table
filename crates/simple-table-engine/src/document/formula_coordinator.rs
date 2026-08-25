use crate::document_data::DocumentData;
use formualizer_parse::parser::ReferenceType;

use crate::document::backing::workbook_patch::StructurePatchDiagnostics;
use crate::domain::{AppliedOperation, CellValue, DocumentCellChange};
use crate::error::AppError;
use crate::formula::ast::FormulaAstService;
use crate::formula::cell_ref::FormulaCellRef;
use crate::formula::engine::FormulaRuntime;
use crate::formula::sheet_name::sheet_names_equal;
use crate::formula::status::{FormulaDiagnostics, FormulaStatus};

pub(crate) struct FormulaCoordinator {
    runtime: FormulaRuntime,
    ast_service: FormulaAstService,
    status: FormulaStatus,
    pending_structure_diagnostics: StructurePatchDiagnostics,
}

pub(crate) const MAX_FORMULA_EVALUATIONS_PER_MUTATION: usize = 16_384;
pub(crate) const MAX_FORMULA_EVALUATION_SOURCE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(crate) struct FormulaWorkLimits {
    pub(crate) max_evaluations: usize,
    pub(crate) max_source_bytes: usize,
}

impl Default for FormulaWorkLimits {
    fn default() -> Self {
        Self {
            max_evaluations: MAX_FORMULA_EVALUATIONS_PER_MUTATION,
            max_source_bytes: MAX_FORMULA_EVALUATION_SOURCE_BYTES,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FormulaWorkEstimate {
    evaluations: usize,
    source_bytes: usize,
}

impl FormulaCoordinator {
    pub(crate) fn new(projection: &mut DocumentData) -> Self {
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

    pub(crate) fn estimated_bytes(&self, projection: &DocumentData) -> usize {
        std::mem::size_of::<Self>()
            + self.runtime.estimated_bytes(projection)
            + self.ast_service.estimated_bytes()
            + formula_status_estimated_bytes(&self.status)
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
        if matches!(&self.status, FormulaStatus::Degraded { .. }) {
            limitations.push("degraded formula runtime".to_string());
        }
        if diagnostics.invalid_formula_count > 0 {
            limitations.push("unparseable formulas".to_string());
        }
        if diagnostics.unsupported_dependency_count > 0 {
            limitations.push("unsupported formula references".to_string());
        }
        limitations
    }

    pub(crate) fn structure_memento_sheet_indexes(
        &mut self,
        projection: &DocumentData,
        operation: &AppliedOperation,
    ) -> Vec<usize> {
        if !operation.impact().is_structure_change() {
            return Vec::new();
        }
        if matches!(self.status, FormulaStatus::Degraded { .. }) {
            return formula_sheet_indexes(self.formula_cell_positions(projection));
        }

        let Some(target) = FormulaStructureTarget::from_operation(projection, operation) else {
            return Vec::new();
        };
        formula_sheets_referencing_target(projection, &target, &mut self.ast_service)
    }

    pub(crate) fn impacted_cells_for_memento(
        &self,
        changed_cells: impl IntoIterator<Item = FormulaCellRef>,
        projection: &DocumentData,
    ) -> Vec<FormulaCellRef> {
        let changed_cells: Vec<FormulaCellRef> = changed_cells.into_iter().collect();
        match &self.status {
            FormulaStatus::Ready { .. } => self
                .runtime
                .impacted_formula_cells_for(changed_cells.iter().copied()),
            FormulaStatus::Degraded { .. } => self.formula_cell_positions(projection),
        }
    }

    pub(crate) fn validate_recalculation_work(
        &self,
        operation: &AppliedOperation,
        projection: &DocumentData,
        limits: FormulaWorkLimits,
    ) -> Result<(), AppError> {
        validate_formula_work_estimate(self.recalculation_work(operation, projection), limits)
    }

    fn recalculation_work(
        &self,
        operation: &AppliedOperation,
        projection: &DocumentData,
    ) -> FormulaWorkEstimate {
        let mut prospective_formulas = std::collections::HashMap::new();
        let mut positions: std::collections::HashSet<FormulaCellRef> = match operation {
            AppliedOperation::SetCell {
                sheet_index,
                row,
                col,
                new_value,
                ..
            } => {
                let changed = FormulaCellRef {
                    sheet_index: *sheet_index,
                    row: *row,
                    col: *col,
                };
                if let CellValue::Formula { formula, .. } = new_value {
                    prospective_formulas.insert(changed, formula.as_str());
                }
                self.impacted_cells_for_memento([changed], projection)
                    .into_iter()
                    .collect()
            }
            AppliedOperation::SetCells { changes } => {
                let changed: Vec<_> = changes
                    .iter()
                    .map(|change| {
                        let cell_ref = FormulaCellRef {
                            sheet_index: change.sheet_index,
                            row: change.row,
                            col: change.col,
                        };
                        if let CellValue::Formula { formula, .. } = &change.new_value {
                            prospective_formulas.insert(cell_ref, formula.as_str());
                        }
                        cell_ref
                    })
                    .collect();
                self.impacted_cells_for_memento(changed, projection)
                    .into_iter()
                    .collect()
            }
            AppliedOperation::SetColumnWidth { .. } | AppliedOperation::SetRowHeight { .. } => {
                std::collections::HashSet::new()
            }
            AppliedOperation::InsertImage { .. }
            | AppliedOperation::UpdateImage { .. }
            | AppliedOperation::DeleteImage { .. } => std::collections::HashSet::new(),
            AppliedOperation::AddRow { .. }
            | AppliedOperation::DeleteRow { .. }
            | AppliedOperation::AddColumn { .. }
            | AppliedOperation::DeleteColumn { .. }
            | AppliedOperation::AddSheet { .. }
            | AppliedOperation::DeleteSheet { .. }
            | AppliedOperation::SortRows(_) => self
                .formula_cell_positions(projection)
                .into_iter()
                .collect(),
        };
        positions.extend(prospective_formulas.keys().copied());
        let source_bytes = positions
            .iter()
            .map(|cell_ref| {
                prospective_formulas.get(cell_ref).map_or_else(
                    || formula_source_at(projection, *cell_ref).map_or(0, str::len),
                    |formula| formula.len(),
                )
            })
            .fold(0usize, usize::saturating_add);
        FormulaWorkEstimate {
            evaluations: positions.len(),
            source_bytes,
        }
    }

    pub(crate) fn recalculate_after_operation(
        &mut self,
        operation: &AppliedOperation,
        projection: &mut DocumentData,
    ) -> Vec<DocumentCellChange> {
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

    pub(crate) fn rebuild(&mut self, projection: &mut DocumentData) -> Vec<DocumentCellChange> {
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

    fn formula_cell_positions(&self, projection: &DocumentData) -> Vec<FormulaCellRef> {
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
        projection: &mut DocumentData,
        error: String,
    ) -> Vec<DocumentCellChange> {
        let mut changes = Vec::new();
        for (sheet_index, sheet) in projection.sheets.iter_mut().enumerate() {
            for (row, row_data) in sheet.rows.iter_mut().enumerate() {
                for (col, cell) in row_data.iter_mut().enumerate() {
                    if !matches!(cell, CellValue::Formula { .. }) {
                        continue;
                    }
                    *cell = cell.with_formula_result(CellValue::Null, Some(error.clone()));
                    changes.push(DocumentCellChange::new(sheet_index, row, col, cell.clone()));
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

fn formula_source_at(file_data: &DocumentData, cell_ref: FormulaCellRef) -> Option<&str> {
    match file_data
        .sheets
        .get(cell_ref.sheet_index)?
        .rows
        .get(cell_ref.row)?
        .get(cell_ref.col)?
    {
        CellValue::Formula { formula, .. } => Some(formula),
        _ => None,
    }
}

fn validate_formula_work_estimate(
    estimate: FormulaWorkEstimate,
    limits: FormulaWorkLimits,
) -> Result<(), AppError> {
    if estimate.evaluations > limits.max_evaluations {
        return Err(AppError::ResourceLimitExceeded(format!(
            "formula recalculation would evaluate {} formulas; the maximum per mutation is {}",
            estimate.evaluations, limits.max_evaluations
        )));
    }
    if estimate.source_bytes > limits.max_source_bytes {
        return Err(AppError::ResourceLimitExceeded(format!(
            "formula recalculation would process {} source bytes; the maximum per mutation is {} bytes",
            estimate.source_bytes, limits.max_source_bytes
        )));
    }
    Ok(())
}

fn formula_status_estimated_bytes(status: &FormulaStatus) -> usize {
    let (message_bytes, diagnostics) = match status {
        FormulaStatus::Ready { diagnostics } => (0, diagnostics),
        FormulaStatus::Degraded {
            message,
            diagnostics,
        } => (message.capacity(), diagnostics),
    };
    message_bytes
        + diagnostics
            .issues
            .iter()
            .map(|issue| std::mem::size_of_val(issue) + issue.message.capacity())
            .sum::<usize>()
}

struct FormulaStructureTarget<'a> {
    sheet_index: usize,
    sheet_name: &'a str,
    include_implicit_current_sheet_refs: bool,
}

impl<'a> FormulaStructureTarget<'a> {
    fn from_operation(projection: &'a DocumentData, operation: &AppliedOperation) -> Option<Self> {
        match operation {
            AppliedOperation::AddRow { sheet_index, .. }
            | AppliedOperation::DeleteRow { sheet_index, .. }
            | AppliedOperation::AddColumn { sheet_index, .. }
            | AppliedOperation::DeleteColumn { sheet_index, .. } => {
                let sheet = projection.sheets.get(*sheet_index)?;
                Some(Self {
                    sheet_index: *sheet_index,
                    sheet_name: &sheet.name,
                    include_implicit_current_sheet_refs: true,
                })
            }
            AppliedOperation::DeleteSheet { sheet_index } => {
                let sheet = projection.sheets.get(*sheet_index)?;
                Some(Self {
                    sheet_index: *sheet_index,
                    sheet_name: &sheet.name,
                    include_implicit_current_sheet_refs: false,
                })
            }
            AppliedOperation::AddSheet { .. } => None,
            AppliedOperation::SetCell { .. }
            | AppliedOperation::SetCells { .. }
            | AppliedOperation::SetColumnWidth { .. }
            | AppliedOperation::SetRowHeight { .. }
            | AppliedOperation::InsertImage { .. }
            | AppliedOperation::UpdateImage { .. }
            | AppliedOperation::DeleteImage { .. }
            | AppliedOperation::SortRows(_) => None,
        }
    }
}

fn formula_sheets_referencing_target(
    projection: &DocumentData,
    target: &FormulaStructureTarget<'_>,
    ast_service: &mut FormulaAstService,
) -> Vec<usize> {
    let mut sheet_indexes = std::collections::BTreeSet::new();
    for (sheet_index, sheet) in projection.sheets.iter().enumerate() {
        'formula_scan: for row in &sheet.rows {
            for cell in row {
                let CellValue::Formula { formula, .. } = cell else {
                    continue;
                };
                if formula_references_target_sheet(ast_service, formula, sheet_index, target) {
                    sheet_indexes.insert(sheet_index);
                    break 'formula_scan;
                }
            }
        }
    }
    sheet_indexes.into_iter().collect()
}

fn formula_references_target_sheet(
    ast_service: &mut FormulaAstService,
    formula: &str,
    formula_sheet_index: usize,
    target: &FormulaStructureTarget<'_>,
) -> bool {
    let Ok(parsed) = ast_service.parse(formula) else {
        return true;
    };

    parsed
        .references()
        .into_iter()
        .any(|reference| reference_targets_sheet(reference, formula_sheet_index, target))
}

fn reference_targets_sheet(
    reference: ReferenceType,
    formula_sheet_index: usize,
    target: &FormulaStructureTarget<'_>,
) -> bool {
    match reference {
        ReferenceType::Cell { sheet, .. } | ReferenceType::Range { sheet, .. } => sheet
            .as_deref()
            .map(|name| sheet_names_equal(name, target.sheet_name))
            .unwrap_or(
                target.include_implicit_current_sheet_refs
                    && formula_sheet_index == target.sheet_index,
            ),
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
            sheet_names_equal(&sheet_first, target.sheet_name)
                || sheet_names_equal(&sheet_last, target.sheet_name)
        }
        ReferenceType::External(_) | ReferenceType::Table(_) | ReferenceType::NamedRange(_) => {
            false
        }
    }
}

fn formula_sheet_indexes(cells: Vec<FormulaCellRef>) -> Vec<usize> {
    let mut sheet_indexes = std::collections::BTreeSet::new();
    for cell in cells {
        sheet_indexes.insert(cell.sheet_index);
    }
    sheet_indexes.into_iter().collect()
}

fn formula_error_change(
    projection: &mut DocumentData,
    sheet_index: usize,
    row: usize,
    col: usize,
    value: &CellValue,
    error: String,
) -> Vec<DocumentCellChange> {
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
    vec![DocumentCellChange::new(sheet_index, row, col, cell.clone())]
}

fn append_unique_changes(target: &mut Vec<DocumentCellChange>, changes: Vec<DocumentCellChange>) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_data::DocumentSheet;
    use crate::domain::CellValue;

    fn sheet(name: &str, rows: Vec<Vec<CellValue>>) -> DocumentSheet {
        DocumentSheet {
            name: name.to_string(),
            rows,
            ..Default::default()
        }
    }

    #[test]
    fn structure_memento_only_includes_formula_sheets_referencing_target() {
        let mut projection = DocumentData {
            path: String::new(),
            file_name: "formulas.xlsx".to_string(),
            sheets: vec![
                sheet("Inputs", vec![vec![CellValue::String("1".to_string())]]),
                sheet(
                    "Calc",
                    vec![vec![CellValue::formula("=A1+Inputs!A1", CellValue::Null)]],
                ),
                sheet(
                    "Other",
                    vec![vec![CellValue::formula("=Calc!A1+1", CellValue::Null)]],
                ),
            ],
        };
        let mut coordinator = FormulaCoordinator::new(&mut projection);

        assert_eq!(
            coordinator.structure_memento_sheet_indexes(
                &projection,
                &AppliedOperation::AddRow {
                    sheet_index: 0,
                    row_index: 0,
                    row_data: Vec::new(),
                    row_height: None,
                },
            ),
            vec![1]
        );
        assert_eq!(
            coordinator.structure_memento_sheet_indexes(
                &projection,
                &AppliedOperation::AddRow {
                    sheet_index: 1,
                    row_index: 0,
                    row_data: Vec::new(),
                    row_height: None,
                },
            ),
            vec![1, 2]
        );
    }

    #[test]
    fn degraded_formula_runtime_blocks_formula_dependent_structure_edits() {
        let mut projection = DocumentData {
            path: String::new(),
            file_name: "formulas.xlsx".to_string(),
            sheets: vec![sheet("Sheet1", Vec::new())],
        };
        let mut coordinator = FormulaCoordinator::new(&mut projection);

        coordinator.mark_degraded("formula budget exceeded".to_string());

        assert!(
            coordinator
                .structure_formula_limitations()
                .contains(&"degraded formula runtime".to_string())
        );
    }

    #[test]
    fn formula_work_rejects_evaluation_count_above_the_limit() {
        assert!(matches!(
            validate_formula_work_estimate(
                FormulaWorkEstimate {
                    evaluations: 3,
                    source_bytes: 12,
                },
                FormulaWorkLimits {
                    max_evaluations: 2,
                    max_source_bytes: 100,
                },
            ),
            Err(AppError::ResourceLimitExceeded(_))
        ));
    }

    #[test]
    fn formula_work_rejects_source_bytes_above_the_limit() {
        assert!(matches!(
            validate_formula_work_estimate(
                FormulaWorkEstimate {
                    evaluations: 1,
                    source_bytes: 101,
                },
                FormulaWorkLimits {
                    max_evaluations: 2,
                    max_source_bytes: 100,
                },
            ),
            Err(AppError::ResourceLimitExceeded(_))
        ));
    }

    #[test]
    fn formula_work_accounts_for_a_new_formula_before_it_is_registered() {
        let mut projection = DocumentData {
            path: String::new(),
            file_name: "new-formula.xlsx".to_string(),
            sheets: vec![sheet(
                "Sheet1",
                vec![vec![CellValue::String("1".to_string())]],
            )],
        };
        let coordinator = FormulaCoordinator::new(&mut projection);
        let operation = AppliedOperation::SetCell {
            sheet_index: 0,
            row: 0,
            col: 0,
            old_value: CellValue::String("1".to_string()),
            new_value: CellValue::formula("=1+1", CellValue::Null),
        };

        assert!(matches!(
            coordinator.validate_recalculation_work(
                &operation,
                &projection,
                FormulaWorkLimits {
                    max_evaluations: 0,
                    max_source_bytes: usize::MAX,
                },
            ),
            Err(AppError::ResourceLimitExceeded(_))
        ));
    }
}
