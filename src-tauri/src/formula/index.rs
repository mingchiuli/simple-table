use crate::document_data::DocumentData;
use std::collections::{HashMap, HashSet};

use formualizer_parse::parser::ReferenceType;

use crate::domain::CellValue;
use crate::formula::ast::{FormulaAstService, MAX_FORMULA_REFERENCES};
use crate::formula::cell_ref::FormulaCellRef;
use crate::formula::sheet_name::sheet_name_key;
use crate::formula::status::{FormulaDiagnostics, FormulaIssue, FormulaIssueKind};

const LARGE_RANGE_ROW_THRESHOLD: usize = 512;
const LARGE_RANGE_COLUMN_THRESHOLD: usize = 128;
const LARGE_RANGE_CELL_THRESHOLD: usize = 65_536;
const RANGE_BUCKET_SIZE: usize = 128;
const LARGE_RANGE_ROW_BUCKET_SIZE: usize = 4096;
const LARGE_RANGE_COLUMN_BUCKET_SIZE: usize = 256;
const MAX_FORMULA_DEPENDENCY_INDEX_BYTES: usize = 32 * 1024 * 1024;
const MAX_FORMULA_DIAGNOSTIC_ISSUES: usize = 100;
const DIRECT_DEPENDENCY_ESTIMATED_BYTES: usize = 96;
const RANGE_DEPENDENCY_ESTIMATED_BYTES: usize = 64;
const BUCKET_REFERENCE_ESTIMATED_BYTES: usize = 16;

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
    sources_by_dependent: HashMap<FormulaCellRef, Vec<FormulaCellRef>>,
    pub(crate) range_dependents: FormulaRangeDependencyIndex,
    pub(crate) large_range_dependents: FormulaLargeRangeDependencyIndex,
    pub(crate) always_recalculate: HashSet<FormulaCellRef>,
    pub(crate) diagnostics: FormulaDiagnostics,
    formula_diagnostics: HashMap<FormulaCellRef, FormulaDependencyDiagnostics>,
    dependency_estimated_bytes: usize,
}

#[derive(Clone, Debug, Default)]
struct FormulaDependencyDiagnostics {
    volatile_formula_count: usize,
    unsupported_dependency_count: usize,
    large_range_dependency_count: usize,
    dependency_estimated_bytes: usize,
    issues: Vec<FormulaIssue>,
}

#[derive(Default)]
pub(crate) struct FormulaRangeDependencyIndex {
    sheets: HashMap<usize, SheetRangeDependencyIndex>,
}

#[derive(Default)]
pub(crate) struct FormulaLargeRangeDependencyIndex {
    sheets: HashMap<usize, SheetLargeRangeDependencyIndex>,
}

#[derive(Clone, Copy)]
enum LargeRangeBucketAxis {
    Row,
    Column,
}

#[derive(Default)]
struct SheetLargeRangeDependencyIndex {
    dependencies: Vec<(FormulaRangeRef, FormulaCellRef)>,
    row_buckets: HashMap<usize, Vec<usize>>,
    column_buckets: HashMap<usize, Vec<usize>>,
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

    fn remove_dependents(&mut self, dependents: &HashSet<FormulaCellRef>) {
        for sheet in self.sheets.values_mut() {
            sheet
                .dependencies
                .retain(|(_, existing)| !dependents.contains(existing));
            sheet.rebuild_buckets();
        }
        self.sheets
            .retain(|_, sheet| !sheet.dependencies.is_empty());
    }
}

impl FormulaLargeRangeDependencyIndex {
    fn insert(&mut self, range: FormulaRangeRef, dependent: FormulaCellRef) {
        let sheet = self.sheets.entry(range.sheet_index).or_default();
        let dependency_index = sheet.dependencies.len();
        sheet.dependencies.push((range, dependent));
        match large_range_bucket_axis(range) {
            LargeRangeBucketAxis::Row => {
                for row_bucket in large_row_bucket_span(range.start_row, range.end_row) {
                    sheet
                        .row_buckets
                        .entry(row_bucket)
                        .or_default()
                        .push(dependency_index);
                }
            }
            LargeRangeBucketAxis::Column => {
                for column_bucket in large_column_bucket_span(range.start_col, range.end_col) {
                    sheet
                        .column_buckets
                        .entry(column_bucket)
                        .or_default()
                        .push(dependency_index);
                }
            }
        }
    }

    pub(crate) fn dependents_for(&self, source: FormulaCellRef) -> Vec<FormulaCellRef> {
        let Some(sheet) = self.sheets.get(&source.sheet_index) else {
            return Vec::new();
        };
        let mut dependents = Vec::new();
        let mut seen = HashSet::new();
        let mut seen_dependencies = HashSet::new();
        collect_large_range_dependents(
            sheet,
            sheet.row_buckets.get(&large_row_bucket_index(source.row)),
            source,
            &mut seen_dependencies,
            &mut seen,
            &mut dependents,
        );
        collect_large_range_dependents(
            sheet,
            sheet
                .column_buckets
                .get(&large_column_bucket_index(source.col)),
            source,
            &mut seen_dependencies,
            &mut seen,
            &mut dependents,
        );
        dependents
    }

    fn remove_dependents(&mut self, dependents: &HashSet<FormulaCellRef>) {
        for sheet in self.sheets.values_mut() {
            sheet
                .dependencies
                .retain(|(_, existing)| !dependents.contains(existing));
            sheet.rebuild_buckets();
        }
        self.sheets
            .retain(|_, sheet| !sheet.dependencies.is_empty());
    }
}

impl SheetLargeRangeDependencyIndex {
    fn rebuild_buckets(&mut self) {
        self.row_buckets.clear();
        self.column_buckets.clear();
        for (dependency_index, (range, _)) in self.dependencies.iter().enumerate() {
            match large_range_bucket_axis(*range) {
                LargeRangeBucketAxis::Row => {
                    for row_bucket in large_row_bucket_span(range.start_row, range.end_row) {
                        self.row_buckets
                            .entry(row_bucket)
                            .or_default()
                            .push(dependency_index);
                    }
                }
                LargeRangeBucketAxis::Column => {
                    for column_bucket in large_column_bucket_span(range.start_col, range.end_col) {
                        self.column_buckets
                            .entry(column_bucket)
                            .or_default()
                            .push(dependency_index);
                    }
                }
            }
        }
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
    pub(crate) fn estimated_bytes(&self) -> usize {
        let direct_dependents = self
            .dependents_by_source
            .values()
            .map(|dependents| {
                std::mem::size_of::<HashSet<FormulaCellRef>>()
                    + dependents.capacity() * std::mem::size_of::<FormulaCellRef>()
            })
            .sum::<usize>();
        let reverse_dependents = self
            .sources_by_dependent
            .values()
            .map(|sources| {
                std::mem::size_of::<Vec<FormulaCellRef>>()
                    + sources.capacity() * std::mem::size_of::<FormulaCellRef>()
            })
            .sum::<usize>();
        let range_dependents = self
            .range_dependents
            .sheets
            .values()
            .map(|sheet| {
                sheet.dependencies.capacity()
                    * std::mem::size_of::<(FormulaRangeRef, FormulaCellRef)>()
                    + sheet
                        .buckets
                        .values()
                        .map(|entries| entries.capacity() * std::mem::size_of::<usize>())
                        .sum::<usize>()
                    + sheet.buckets.capacity() * 32
            })
            .sum::<usize>();
        let large_range_dependents = self
            .large_range_dependents
            .sheets
            .values()
            .map(|sheet| {
                sheet.dependencies.capacity()
                    * std::mem::size_of::<(FormulaRangeRef, FormulaCellRef)>()
                    + sheet
                        .row_buckets
                        .values()
                        .chain(sheet.column_buckets.values())
                        .map(|entries| entries.capacity() * std::mem::size_of::<usize>())
                        .sum::<usize>()
                    + (sheet.row_buckets.capacity() + sheet.column_buckets.capacity()) * 32
            })
            .sum::<usize>();
        let diagnostic_bytes = self
            .formula_diagnostics
            .values()
            .flat_map(|diagnostics| diagnostics.issues.iter())
            .map(|issue| std::mem::size_of_val(issue) + issue.message.capacity())
            .sum::<usize>();

        std::mem::size_of::<Self>()
            + self.formulas.capacity() * std::mem::size_of::<FormulaCellRef>()
            + self.dependents_by_source.capacity() * 64
            + direct_dependents
            + self.sources_by_dependent.capacity() * 48
            + reverse_dependents
            + range_dependents
            + large_range_dependents
            + self.always_recalculate.capacity() * std::mem::size_of::<FormulaCellRef>()
            + self.formula_diagnostics.capacity() * 64
            + diagnostic_bytes
    }

    pub(crate) fn update_formula_dependencies(
        &mut self,
        file_data: &DocumentData,
        formula_refs: impl IntoIterator<Item = FormulaCellRef>,
        registered_formulas: &HashSet<FormulaCellRef>,
        ast_service: &mut FormulaAstService,
    ) {
        let formula_refs: HashSet<_> = formula_refs.into_iter().collect();
        self.remove_formulas(&formula_refs);
        let sheet_indexes = sheet_indexes(file_data);
        for formula_ref in formula_refs {
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
        file_data: &DocumentData,
        formula_ref: FormulaCellRef,
        registered_formulas: &HashSet<FormulaCellRef>,
        sheet_indexes: &HashMap<String, usize>,
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
                diagnostics.issues.push(formula_issue(
                    formula_ref,
                    FormulaIssueKind::VolatileFormula,
                    "Formula uses volatile functions and is recalculated after every edit",
                ));
                self.always_recalculate.insert(formula_ref);
                self.add_formula_diagnostics(formula_ref, diagnostics);
                return;
            }
            DependencyCollection::Unsupported => {
                diagnostics.unsupported_dependency_count = 1;
                diagnostics.issues.push(formula_issue(
                    formula_ref,
                    FormulaIssueKind::UnsupportedDependency,
                    "Formula contains references that cannot be tracked precisely",
                ));
                self.always_recalculate.insert(formula_ref);
                self.add_formula_diagnostics(formula_ref, diagnostics);
                return;
            }
        };

        let dependency_estimated_bytes = estimate_formula_dependency_bytes(&dependencies);
        if self
            .dependency_estimated_bytes
            .saturating_add(dependency_estimated_bytes)
            > MAX_FORMULA_DEPENDENCY_INDEX_BYTES
        {
            diagnostics.unsupported_dependency_count = 1;
            diagnostics.issues.push(formula_issue(
                formula_ref,
                FormulaIssueKind::UnsupportedDependency,
                "Formula dependency tracking exceeded its memory budget; the formula will be recalculated after every edit",
            ));
            self.always_recalculate.insert(formula_ref);
            self.add_formula_diagnostics(formula_ref, diagnostics);
            return;
        }
        diagnostics.dependency_estimated_bytes = dependency_estimated_bytes;
        self.dependency_estimated_bytes = self
            .dependency_estimated_bytes
            .saturating_add(dependency_estimated_bytes);

        let direct_sources = dependencies.cells.iter().copied().collect::<Vec<_>>();
        for dependency in dependencies.cells {
            self.dependents_by_source
                .entry(dependency)
                .or_default()
                .insert(formula_ref);
        }
        if !direct_sources.is_empty() {
            self.sources_by_dependent
                .insert(formula_ref, direct_sources);
        }
        for dependency in dependencies.ranges {
            if dependency.is_large() {
                diagnostics.large_range_dependency_count += 1;
                self.large_range_dependents.insert(dependency, formula_ref);
                continue;
            }
            self.range_dependents.insert(dependency, formula_ref);
        }
        if diagnostics.large_range_dependency_count > 0 {
            diagnostics.issues.push(formula_issue(
                formula_ref,
                FormulaIssueKind::LargeRangeDependency,
                format!(
                    "Formula depends on {} large range(s); dependency tracking uses coarse buckets",
                    diagnostics.large_range_dependency_count
                ),
            ));
        }
        self.add_formula_diagnostics(formula_ref, diagnostics);
    }

    fn remove_formulas(&mut self, formula_refs: &HashSet<FormulaCellRef>) {
        for formula_ref in formula_refs {
            self.formulas.remove(formula_ref);
            self.always_recalculate.remove(formula_ref);
            if let Some(sources) = self.sources_by_dependent.remove(formula_ref) {
                for source in sources {
                    if let Some(dependents) = self.dependents_by_source.get_mut(&source) {
                        dependents.remove(formula_ref);
                    }
                }
            }
            if let Some(diagnostics) = self.formula_diagnostics.remove(formula_ref) {
                self.subtract_diagnostics(diagnostics);
            }
        }
        self.dependents_by_source
            .retain(|_, dependents| !dependents.is_empty());
        self.range_dependents.remove_dependents(formula_refs);
        self.large_range_dependents.remove_dependents(formula_refs);
    }

    fn add_formula_diagnostics(
        &mut self,
        formula_ref: FormulaCellRef,
        diagnostics: FormulaDependencyDiagnostics,
    ) {
        self.diagnostics.volatile_formula_count += diagnostics.volatile_formula_count;
        self.diagnostics.unsupported_dependency_count += diagnostics.unsupported_dependency_count;
        self.diagnostics.large_range_dependency_count += diagnostics.large_range_dependency_count;
        let remaining_issue_capacity =
            MAX_FORMULA_DIAGNOSTIC_ISSUES.saturating_sub(self.diagnostics.issues.len());
        self.diagnostics.issues.extend(
            diagnostics
                .issues
                .iter()
                .take(remaining_issue_capacity)
                .cloned(),
        );
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
        self.dependency_estimated_bytes = self
            .dependency_estimated_bytes
            .saturating_sub(diagnostics.dependency_estimated_bytes);
        self.diagnostics
            .issues
            .retain(|issue| !diagnostics.issues.contains(issue));
    }
}

pub(crate) fn build_dependency_index(
    file_data: &DocumentData,
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

fn sheet_indexes(file_data: &DocumentData) -> HashMap<String, usize> {
    let mut indexes = HashMap::new();
    for (sheet_index, sheet) in file_data.sheets.iter().enumerate() {
        indexes
            .entry(sheet_name_key(&sheet.name))
            .or_insert(sheet_index);
    }
    indexes
}

fn bucket_index(index: usize) -> usize {
    index / RANGE_BUCKET_SIZE
}

fn bucket_span(start: usize, end: usize) -> std::ops::RangeInclusive<usize> {
    bucket_index(start)..=bucket_index(end)
}

fn large_row_bucket_index(index: usize) -> usize {
    index / LARGE_RANGE_ROW_BUCKET_SIZE
}

fn large_column_bucket_index(index: usize) -> usize {
    index / LARGE_RANGE_COLUMN_BUCKET_SIZE
}

fn large_row_bucket_span(start: usize, end: usize) -> std::ops::RangeInclusive<usize> {
    large_row_bucket_index(start)..=large_row_bucket_index(end)
}

fn large_column_bucket_span(start: usize, end: usize) -> std::ops::RangeInclusive<usize> {
    large_column_bucket_index(start)..=large_column_bucket_index(end)
}

fn large_range_bucket_axis(range: FormulaRangeRef) -> LargeRangeBucketAxis {
    let row_bucket_count = large_row_bucket_index(range.end_row)
        .saturating_sub(large_row_bucket_index(range.start_row))
        + 1;
    let column_bucket_count = large_column_bucket_index(range.end_col)
        .saturating_sub(large_column_bucket_index(range.start_col))
        + 1;
    if row_bucket_count <= column_bucket_count {
        LargeRangeBucketAxis::Row
    } else {
        LargeRangeBucketAxis::Column
    }
}

fn collect_large_range_dependents(
    sheet: &SheetLargeRangeDependencyIndex,
    dependency_indexes: Option<&Vec<usize>>,
    source: FormulaCellRef,
    seen_dependencies: &mut HashSet<usize>,
    seen_dependents: &mut HashSet<FormulaCellRef>,
    dependents: &mut Vec<FormulaCellRef>,
) {
    let Some(dependency_indexes) = dependency_indexes else {
        return;
    };
    for dependency_index in dependency_indexes {
        if !seen_dependencies.insert(*dependency_index) {
            continue;
        }
        let Some((range, dependent)) = sheet.dependencies.get(*dependency_index) else {
            continue;
        };
        if range.contains(source) && seen_dependents.insert(*dependent) {
            dependents.push(*dependent);
        }
    }
}

pub(crate) fn unregistered_formula_diagnostics(
    file_data: &DocumentData,
    registered_formulas: &HashSet<FormulaCellRef>,
) -> (usize, Vec<FormulaIssue>) {
    let mut count = 0usize;
    let mut issues = Vec::new();
    for (sheet_index, sheet) in file_data.sheets.iter().enumerate() {
        for (row, row_data) in sheet.rows.iter().enumerate() {
            for (col, cell) in row_data.iter().enumerate() {
                let formula_ref = FormulaCellRef {
                    sheet_index,
                    row,
                    col,
                };
                if !matches!(cell, CellValue::Formula { .. })
                    || registered_formulas.contains(&formula_ref)
                {
                    continue;
                }
                count = count.saturating_add(1);
                if issues.len() < MAX_FORMULA_DIAGNOSTIC_ISSUES {
                    issues.push(formula_issue(
                        formula_ref,
                        FormulaIssueKind::InvalidFormula,
                        "Formula could not be parsed or registered",
                    ));
                }
            }
        }
    }
    (count, issues)
}

fn formula_issue(
    formula_ref: FormulaCellRef,
    kind: FormulaIssueKind,
    message: impl Into<String>,
) -> FormulaIssue {
    FormulaIssue::new(
        formula_ref.sheet_index,
        formula_ref.row,
        formula_ref.col,
        kind,
        message,
    )
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
    sheet_indexes: &HashMap<String, usize>,
    ast_service: &mut FormulaAstService,
) -> DependencyCollection {
    let Ok(ast) = ast_service.parse(formula) else {
        return DependencyCollection::Unsupported;
    };
    if ast.contains_volatile() {
        return DependencyCollection::Volatile;
    }
    if ast.reference_count() > MAX_FORMULA_REFERENCES {
        return DependencyCollection::Unsupported;
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

fn estimate_formula_dependency_bytes(dependencies: &FormulaDependencies) -> usize {
    let direct_bytes = dependencies
        .cells
        .len()
        .saturating_mul(DIRECT_DEPENDENCY_ESTIMATED_BYTES);
    dependencies
        .ranges
        .iter()
        .fold(direct_bytes, |total, range| {
            let bucket_references = if range.is_large() {
                match large_range_bucket_axis(*range) {
                    LargeRangeBucketAxis::Row => large_row_bucket_index(range.end_row)
                        .saturating_sub(large_row_bucket_index(range.start_row))
                        .saturating_add(1),
                    LargeRangeBucketAxis::Column => large_column_bucket_index(range.end_col)
                        .saturating_sub(large_column_bucket_index(range.start_col))
                        .saturating_add(1),
                }
            } else {
                bucket_index(range.end_row)
                    .saturating_sub(bucket_index(range.start_row))
                    .saturating_add(1)
                    .saturating_mul(
                        bucket_index(range.end_col)
                            .saturating_sub(bucket_index(range.start_col))
                            .saturating_add(1),
                    )
            };
            total
                .saturating_add(RANGE_DEPENDENCY_ESTIMATED_BYTES)
                .saturating_add(bucket_references.saturating_mul(BUCKET_REFERENCE_ESTIMATED_BYTES))
        })
}

fn resolve_reference_sheet(
    sheet_name: Option<&str>,
    current_sheet_index: usize,
    sheet_indexes: &HashMap<String, usize>,
) -> Option<usize> {
    sheet_name
        .map(|name| sheet_indexes.get(&sheet_name_key(name)).copied())
        .unwrap_or(Some(current_sheet_index))
}

fn to_zero_based(index: u32) -> Option<usize> {
    usize::try_from(index.checked_sub(1)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_data::DocumentSheet;

    #[test]
    fn excessive_formula_references_use_dependency_fallback() {
        let formula = format!(
            "=SUM({})",
            (1..=MAX_FORMULA_REFERENCES + 1)
                .map(|row| format!("A{row}"))
                .collect::<Vec<_>>()
                .join(",")
        );
        let mut ast_service = FormulaAstService::new();
        let parsed = ast_service.parse(&formula).expect("parse bounded formula");
        assert_eq!(parsed.reference_count(), MAX_FORMULA_REFERENCES + 1);

        assert!(matches!(
            collect_formula_dependencies(&formula, 0, &HashMap::new(), &mut ast_service),
            DependencyCollection::Unsupported
        ));
    }

    #[test]
    fn batch_dependency_update_removes_old_reverse_edges() {
        let first = FormulaCellRef {
            sheet_index: 0,
            row: 0,
            col: 1,
        };
        let second = FormulaCellRef {
            sheet_index: 0,
            row: 0,
            col: 2,
        };
        let registered = HashSet::from([first, second]);
        let mut file_data = DocumentData {
            path: String::new(),
            file_name: "dependencies.xlsx".to_string(),
            sheets: vec![DocumentSheet {
                name: "Sheet1".to_string(),
                rows: vec![vec![
                    CellValue::Null,
                    CellValue::formula("=A1", CellValue::Null),
                    CellValue::formula("=A1", CellValue::Null),
                    CellValue::Null,
                    CellValue::Null,
                ]],
                ..Default::default()
            }],
        };
        let mut ast_service = FormulaAstService::new();
        let mut index = build_dependency_index(&file_data, &registered, &mut ast_service);

        file_data.sheets[0].rows[0][1] = CellValue::formula("=D1", CellValue::Null);
        file_data.sheets[0].rows[0][2] = CellValue::formula("=E1", CellValue::Null);
        index.update_formula_dependencies(
            &file_data,
            [first, second],
            &registered,
            &mut ast_service,
        );

        let source = |col| FormulaCellRef {
            sheet_index: 0,
            row: 0,
            col,
        };
        assert!(!index.dependents_by_source.contains_key(&source(0)));
        assert!(index.dependents_by_source[&source(3)].contains(&first));
        assert!(index.dependents_by_source[&source(4)].contains(&second));
        assert!(index.dependency_estimated_bytes <= MAX_FORMULA_DEPENDENCY_INDEX_BYTES);
    }
}
