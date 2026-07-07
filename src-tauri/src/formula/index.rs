use std::collections::{HashMap, HashSet};

use formualizer_parse::parser::ReferenceType;

use crate::formula::ast::FormulaAstService;
use crate::formula::cell_ref::FormulaCellRef;
use crate::types::{CellValue, FileData, FormulaDiagnostics};

const LARGE_RANGE_ROW_THRESHOLD: usize = 512;
const LARGE_RANGE_COLUMN_THRESHOLD: usize = 128;
const LARGE_RANGE_CELL_THRESHOLD: usize = 65_536;
const RANGE_BUCKET_SIZE: usize = 128;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct FormulaRangeRef {
    sheet_index: usize,
    start_row: usize,
    start_col: usize,
    end_row: usize,
    end_col: usize,
}

impl FormulaRangeRef {
    fn contains(&self, cell: FormulaCellRef) -> bool {
        self.sheet_index == cell.sheet_index
            && (self.start_row..=self.end_row).contains(&cell.row)
            && (self.start_col..=self.end_col).contains(&cell.col)
    }

    fn row_span(&self) -> usize {
        self.end_row
            .saturating_sub(self.start_row)
            .saturating_add(1)
    }

    fn column_span(&self) -> usize {
        self.end_col
            .saturating_sub(self.start_col)
            .saturating_add(1)
    }

    fn cell_span(&self) -> usize {
        self.row_span().saturating_mul(self.column_span())
    }

    fn is_large(&self) -> bool {
        self.row_span() > LARGE_RANGE_ROW_THRESHOLD
            || self.column_span() > LARGE_RANGE_COLUMN_THRESHOLD
            || self.cell_span() > LARGE_RANGE_CELL_THRESHOLD
    }
}

#[derive(Default)]
pub(crate) struct FormulaDependencyIndex {
    pub(crate) formulas: HashSet<FormulaCellRef>,
    pub(crate) dependents_by_source: HashMap<FormulaCellRef, HashSet<FormulaCellRef>>,
    pub(crate) range_dependents: FormulaRangeDependencyIndex,
    pub(crate) large_range_dependents: FormulaLargeRangeDependencyIndex,
    pub(crate) always_recalculate: HashSet<FormulaCellRef>,
    pub(crate) diagnostics: FormulaDiagnostics,
    formula_diagnostics: HashMap<FormulaCellRef, FormulaDependencyDiagnostics>,
}

#[derive(Clone, Copy, Debug, Default)]
struct FormulaDependencyDiagnostics {
    volatile_formula_count: usize,
    unsupported_dependency_count: usize,
    large_range_dependency_count: usize,
}

#[derive(Default)]
pub(crate) struct FormulaRangeDependencyIndex {
    sheets: HashMap<usize, SheetRangeDependencyIndex>,
}

#[derive(Default)]
pub(crate) struct FormulaLargeRangeDependencyIndex {
    dependencies: Vec<(FormulaRangeRef, FormulaCellRef)>,
}

#[derive(Default)]
struct SheetRangeDependencyIndex {
    dependencies: Vec<(FormulaRangeRef, FormulaCellRef)>,
    buckets: HashMap<(usize, usize), Vec<usize>>,
}

impl FormulaRangeDependencyIndex {
    fn insert(&mut self, range: FormulaRangeRef, dependent: FormulaCellRef) {
        let sheet = self.sheets.entry(range.sheet_index).or_default();

        let dependency_index = sheet.dependencies.len();
        sheet.dependencies.push((range, dependent));
        for row_bucket in bucket_span(range.start_row, range.end_row) {
            for col_bucket in bucket_span(range.start_col, range.end_col) {
                sheet
                    .buckets
                    .entry((row_bucket, col_bucket))
                    .or_default()
                    .push(dependency_index);
            }
        }
    }

    pub(crate) fn dependents_for(&self, source: FormulaCellRef) -> Vec<FormulaCellRef> {
        let Some(sheet) = self.sheets.get(&source.sheet_index) else {
            return Vec::new();
        };
        let mut dependents = Vec::new();
        let mut seen = HashSet::new();
        let bucket = (bucket_index(source.row), bucket_index(source.col));
        if let Some(dependency_indexes) = sheet.buckets.get(&bucket) {
            for dependency_index in dependency_indexes {
                let Some((range, dependent)) = sheet.dependencies.get(*dependency_index) else {
                    continue;
                };
                if range.contains(source) && seen.insert(*dependent) {
                    dependents.push(*dependent);
                }
            }
        }
        dependents
    }

    fn remove_dependent(&mut self, dependent: FormulaCellRef) {
        for sheet in self.sheets.values_mut() {
            sheet
                .dependencies
                .retain(|(_, existing)| *existing != dependent);
            sheet.rebuild_buckets();
        }
        self.sheets
            .retain(|_, sheet| !sheet.dependencies.is_empty());
    }
}

impl FormulaLargeRangeDependencyIndex {
    fn insert(&mut self, range: FormulaRangeRef, dependent: FormulaCellRef) {
        self.dependencies.push((range, dependent));
    }

    pub(crate) fn dependents_for(&self, source: FormulaCellRef) -> Vec<FormulaCellRef> {
        let mut dependents = Vec::new();
        let mut seen = HashSet::new();
        for (range, dependent) in &self.dependencies {
            if range.contains(source) && seen.insert(*dependent) {
                dependents.push(*dependent);
            }
        }
        dependents
    }

    fn remove_dependent(&mut self, dependent: FormulaCellRef) {
        self.dependencies
            .retain(|(_, existing)| *existing != dependent);
    }
}

impl SheetRangeDependencyIndex {
    fn rebuild_buckets(&mut self) {
        self.buckets.clear();
        for (dependency_index, (range, _)) in self.dependencies.iter().enumerate() {
            for row_bucket in bucket_span(range.start_row, range.end_row) {
                for col_bucket in bucket_span(range.start_col, range.end_col) {
                    self.buckets
                        .entry((row_bucket, col_bucket))
                        .or_default()
                        .push(dependency_index);
                }
            }
        }
    }
}

impl FormulaDependencyIndex {
    pub(crate) fn update_formula_dependencies(
        &mut self,
        file_data: &FileData,
        formula_refs: impl IntoIterator<Item = FormulaCellRef>,
        registered_formulas: &HashSet<FormulaCellRef>,
        ast_service: &mut FormulaAstService,
    ) {
        let sheet_indexes = sheet_indexes(file_data);
        for formula_ref in formula_refs {
            self.remove_formula(formula_ref);
            self.insert_registered_formula(
                file_data,
                formula_ref,
                registered_formulas,
                &sheet_indexes,
                ast_service,
            );
        }
    }

    fn insert_registered_formula(
        &mut self,
        file_data: &FileData,
        formula_ref: FormulaCellRef,
        registered_formulas: &HashSet<FormulaCellRef>,
        sheet_indexes: &HashMap<&str, usize>,
        ast_service: &mut FormulaAstService,
    ) {
        if !registered_formulas.contains(&formula_ref) {
            return;
        }
        let Some(CellValue::Formula { formula, .. }) = file_data
            .sheets
            .get(formula_ref.sheet_index)
            .and_then(|sheet| sheet.rows.get(formula_ref.row))
            .and_then(|row| row.get(formula_ref.col))
        else {
            return;
        };

        self.formulas.insert(formula_ref);
        let mut diagnostics = FormulaDependencyDiagnostics::default();
        let dependencies = match collect_formula_dependencies(
            formula,
            formula_ref.sheet_index,
            sheet_indexes,
            ast_service,
        ) {
            DependencyCollection::Precise(dependencies) => dependencies,
            DependencyCollection::Volatile => {
                diagnostics.volatile_formula_count = 1;
                self.always_recalculate.insert(formula_ref);
                self.add_formula_diagnostics(formula_ref, diagnostics);
                return;
            }
            DependencyCollection::Unsupported => {
                diagnostics.unsupported_dependency_count = 1;
                self.always_recalculate.insert(formula_ref);
                self.add_formula_diagnostics(formula_ref, diagnostics);
                return;
            }
        };

        for dependency in dependencies.cells {
            self.dependents_by_source
                .entry(dependency)
                .or_default()
                .insert(formula_ref);
        }
        for dependency in dependencies.ranges {
            if dependency.is_large() {
                diagnostics.large_range_dependency_count += 1;
                self.large_range_dependents.insert(dependency, formula_ref);
                continue;
            }
            self.range_dependents.insert(dependency, formula_ref);
        }
        self.add_formula_diagnostics(formula_ref, diagnostics);
    }

    fn remove_formula(&mut self, formula_ref: FormulaCellRef) {
        self.formulas.remove(&formula_ref);
        self.always_recalculate.remove(&formula_ref);
        for dependents in self.dependents_by_source.values_mut() {
            dependents.remove(&formula_ref);
        }
        self.dependents_by_source
            .retain(|_, dependents| !dependents.is_empty());
        self.range_dependents.remove_dependent(formula_ref);
        self.large_range_dependents.remove_dependent(formula_ref);
        if let Some(diagnostics) = self.formula_diagnostics.remove(&formula_ref) {
            self.subtract_diagnostics(diagnostics);
        }
    }

    fn add_formula_diagnostics(
        &mut self,
        formula_ref: FormulaCellRef,
        diagnostics: FormulaDependencyDiagnostics,
    ) {
        self.diagnostics.volatile_formula_count += diagnostics.volatile_formula_count;
        self.diagnostics.unsupported_dependency_count += diagnostics.unsupported_dependency_count;
        self.diagnostics.large_range_dependency_count += diagnostics.large_range_dependency_count;
        self.formula_diagnostics.insert(formula_ref, diagnostics);
    }

    fn subtract_diagnostics(&mut self, diagnostics: FormulaDependencyDiagnostics) {
        self.diagnostics.volatile_formula_count = self
            .diagnostics
            .volatile_formula_count
            .saturating_sub(diagnostics.volatile_formula_count);
        self.diagnostics.unsupported_dependency_count = self
            .diagnostics
            .unsupported_dependency_count
            .saturating_sub(diagnostics.unsupported_dependency_count);
        self.diagnostics.large_range_dependency_count = self
            .diagnostics
            .large_range_dependency_count
            .saturating_sub(diagnostics.large_range_dependency_count);
    }
}

pub(crate) fn build_dependency_index(
    file_data: &FileData,
    registered_formulas: &HashSet<FormulaCellRef>,
    ast_service: &mut FormulaAstService,
) -> FormulaDependencyIndex {
    let mut index = FormulaDependencyIndex::default();
    let sheet_indexes = sheet_indexes(file_data);

    for (sheet_index, sheet) in file_data.sheets.iter().enumerate() {
        for (row_idx, row) in sheet.rows.iter().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
                if !matches!(cell, CellValue::Formula { .. }) {
                    continue;
                }
                let formula_ref = FormulaCellRef {
                    sheet_index,
                    row: row_idx,
                    col: col_idx,
                };
                index.insert_registered_formula(
                    file_data,
                    formula_ref,
                    registered_formulas,
                    &sheet_indexes,
                    ast_service,
                );
            }
        }
    }

    index
}

fn sheet_indexes(file_data: &FileData) -> HashMap<&str, usize> {
    file_data
        .sheets
        .iter()
        .enumerate()
        .map(|(sheet_index, sheet)| (sheet.name.as_str(), sheet_index))
        .collect()
}

fn bucket_index(index: usize) -> usize {
    index / RANGE_BUCKET_SIZE
}

fn bucket_span(start: usize, end: usize) -> std::ops::RangeInclusive<usize> {
    bucket_index(start)..=bucket_index(end)
}

pub(crate) fn count_unregistered_formula_cells(
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

#[derive(Default)]
struct FormulaDependencies {
    cells: HashSet<FormulaCellRef>,
    ranges: Vec<FormulaRangeRef>,
}

enum DependencyCollection {
    Precise(FormulaDependencies),
    Volatile,
    Unsupported,
}

fn collect_formula_dependencies(
    formula: &str,
    current_sheet_index: usize,
    sheet_indexes: &HashMap<&str, usize>,
    ast_service: &mut FormulaAstService,
) -> DependencyCollection {
    let Ok(ast) = ast_service.parse(formula) else {
        return DependencyCollection::Unsupported;
    };
    if ast.contains_volatile() {
        return DependencyCollection::Volatile;
    }

    let mut dependencies = FormulaDependencies::default();
    for reference in ast.references() {
        match reference {
            ReferenceType::Cell {
                sheet, row, col, ..
            } => {
                dependencies.cells.insert(FormulaCellRef {
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
                dependencies.ranges.push(FormulaRangeRef {
                    sheet_index,
                    start_row: match to_zero_based(start_row) {
                        Some(row) => row,
                        None => return DependencyCollection::Unsupported,
                    },
                    start_col: match to_zero_based(start_col) {
                        Some(col) => col,
                        None => return DependencyCollection::Unsupported,
                    },
                    end_row: match to_zero_based(end_row) {
                        Some(row) => row,
                        None => return DependencyCollection::Unsupported,
                    },
                    end_col: match to_zero_based(end_col) {
                        Some(col) => col,
                        None => return DependencyCollection::Unsupported,
                    },
                });
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
