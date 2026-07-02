use std::collections::{HashMap, HashSet};

use formualizer_parse::parser::{CollectPolicy, ReferenceType};

use crate::formula::engine::FormulaCellRef;
use crate::types::{CellValue, FileData, FormulaDiagnostics};

const MAX_INDEXED_RANGE_ROWS: usize = 512;

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
}

#[derive(Default)]
pub(crate) struct FormulaDependencyIndex {
    pub(crate) formulas: HashSet<FormulaCellRef>,
    pub(crate) dependents_by_source: HashMap<FormulaCellRef, HashSet<FormulaCellRef>>,
    pub(crate) range_dependents: FormulaRangeDependencyIndex,
    pub(crate) always_recalculate: HashSet<FormulaCellRef>,
    pub(crate) diagnostics: FormulaDiagnostics,
}

#[derive(Default)]
pub(crate) struct FormulaRangeDependencyIndex {
    sheets: HashMap<usize, SheetRangeDependencyIndex>,
}

#[derive(Default)]
struct SheetRangeDependencyIndex {
    dependencies: Vec<(FormulaRangeRef, FormulaCellRef)>,
    rows: HashMap<usize, Vec<usize>>,
    large_dependencies: Vec<(FormulaRangeRef, FormulaCellRef)>,
}

impl FormulaRangeDependencyIndex {
    fn insert(&mut self, range: FormulaRangeRef, dependent: FormulaCellRef) {
        let sheet = self.sheets.entry(range.sheet_index).or_default();
        if range
            .end_row
            .saturating_sub(range.start_row)
            .saturating_add(1)
            > MAX_INDEXED_RANGE_ROWS
        {
            sheet.large_dependencies.push((range, dependent));
            return;
        }

        let dependency_index = sheet.dependencies.len();
        sheet.dependencies.push((range, dependent));
        for row in range.start_row..=range.end_row {
            sheet.rows.entry(row).or_default().push(dependency_index);
        }
    }

    pub(crate) fn dependents_for(&self, source: FormulaCellRef) -> Vec<FormulaCellRef> {
        let Some(sheet) = self.sheets.get(&source.sheet_index) else {
            return Vec::new();
        };
        let mut dependents = Vec::new();
        let mut seen = HashSet::new();
        if let Some(dependency_indexes) = sheet.rows.get(&source.row) {
            for dependency_index in dependency_indexes {
                let Some((range, dependent)) = sheet.dependencies.get(*dependency_index) else {
                    continue;
                };
                if range.contains(source) && seen.insert(*dependent) {
                    dependents.push(*dependent);
                }
            }
        }
        for (range, dependent) in &sheet.large_dependencies {
            if range.contains(source) && seen.insert(*dependent) {
                dependents.push(*dependent);
            }
        }
        dependents
    }
}

pub(crate) fn build_dependency_index(
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

                index.formulas.insert(formula_ref);

                let dependencies =
                    match collect_formula_dependencies(formula, sheet_index, &sheet_indexes) {
                        DependencyCollection::Precise(dependencies) => dependencies,
                        DependencyCollection::Volatile => {
                            index.diagnostics.volatile_formula_count += 1;
                            index.always_recalculate.insert(formula_ref);
                            continue;
                        }
                        DependencyCollection::Unsupported => {
                            index.diagnostics.unsupported_dependency_count += 1;
                            index.always_recalculate.insert(formula_ref);
                            continue;
                        }
                    };

                for dependency in dependencies.cells {
                    index
                        .dependents_by_source
                        .entry(dependency)
                        .or_default()
                        .insert(formula_ref);
                }
                for dependency in dependencies.ranges {
                    if dependency.row_span() > MAX_INDEXED_RANGE_ROWS {
                        index.diagnostics.large_range_dependency_count += 1;
                    }
                    index.range_dependents.insert(dependency, formula_ref);
                }
            }
        }
    }

    index
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
) -> DependencyCollection {
    let Ok(ast) =
        formualizer_parse::parse_with_volatility_classifier(formula, is_volatile_function)
    else {
        return DependencyCollection::Unsupported;
    };
    if ast.contains_volatile() {
        return DependencyCollection::Volatile;
    }

    let mut dependencies = FormulaDependencies::default();
    for reference in ast.collect_references(&CollectPolicy::default()) {
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

fn is_volatile_function(name: &str) -> bool {
    matches!(
        name.to_ascii_uppercase().as_str(),
        "NOW" | "TODAY" | "RAND" | "RANDBETWEEN" | "OFFSET" | "INDIRECT" | "INFO" | "CELL"
    )
}
