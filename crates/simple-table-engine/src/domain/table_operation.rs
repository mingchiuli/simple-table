use std::cmp::Ordering;
use std::collections::HashMap;

use crate::document_data::{DocumentSheet, ImageAnchor};
use crate::domain::CellValue;
use crate::error::AppError;
use crate::formula::ast::FormulaAstService;
use crate::formula::reference_rewrite::translate_formula_for_move;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterOperator {
    Equals,
    NotEquals,
    Contains,
    Blank,
    NotBlank,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellRange {
    pub start_row: usize,
    pub end_row: usize,
    pub start_col: usize,
    pub end_col: usize,
}

impl CellRange {
    pub fn contains(self, row: usize, col: usize) -> bool {
        (self.start_row..=self.end_row).contains(&row)
            && (self.start_col..=self.end_col).contains(&col)
    }

    pub fn body_start_row(self) -> usize {
        self.start_row.saturating_add(1)
    }

    pub fn body_row_count(self) -> usize {
        self.end_row.saturating_sub(self.start_row)
    }
}

#[derive(Clone, Debug)]
pub struct FormulaTextAtCell {
    pub row: usize,
    pub col: usize,
    pub formula: String,
}

#[derive(Clone, Debug)]
pub struct ResolvedSort {
    pub sheet_index: usize,
    pub range: CellRange,
    /// Destination body-row offset to source body-row offset.
    pub permutation: Vec<usize>,
    pub inverse_permutation: Vec<usize>,
    pub before_formulas: Vec<FormulaTextAtCell>,
    pub after_formulas: Vec<FormulaTextAtCell>,
}

pub(crate) fn resolve_sort(
    sheet_index: usize,
    sheet: &DocumentSheet,
    anchor_row: usize,
    anchor_col: usize,
    direction: SortDirection,
) -> Result<ResolvedSort, AppError> {
    let range = current_region(sheet, anchor_row, anchor_col)?;
    if !range.contains(anchor_row, anchor_col) {
        return Err(AppError::DocumentStateInvalid(
            "the sort column must be inside the current data region".to_string(),
        ));
    }
    validate_sort_region(sheet, range)?;

    let body_start = range.body_start_row();
    let body_count = range.body_row_count();
    let sort_keys = (0..body_count)
        .map(|offset| comparable_value(cell_at(sheet, body_start + offset, anchor_col)))
        .collect::<Vec<_>>();
    let mut permutation: Vec<_> = (0..body_count).collect();
    permutation.sort_by(|left, right| {
        compare_comparable_values(&sort_keys[*left], &sort_keys[*right], direction)
    });

    let mut inverse_permutation = vec![0; body_count];
    for (destination, source) in permutation.iter().copied().enumerate() {
        inverse_permutation[source] = destination;
    }

    let mut ast_service = FormulaAstService::new();
    let mut before_formulas = Vec::new();
    let mut after_formulas = Vec::new();
    for (destination, source) in permutation.iter().copied().enumerate() {
        let source_row = body_start + source;
        let destination_row = body_start + destination;
        let row_delta = signed_delta(destination_row, source_row)?;
        for col in range.start_col..=range.end_col {
            let CellValue::Formula { formula, .. } = cell_at(sheet, source_row, col) else {
                continue;
            };
            before_formulas.push(FormulaTextAtCell {
                row: source_row,
                col,
                formula: formula.clone(),
            });
            let translated = translate_formula_for_move(&mut ast_service, formula, row_delta, 0)
                .map_err(|reason| {
                    AppError::DocumentStateInvalid(format!(
                        "formula {} cannot be moved by sort: {reason}",
                        excel_cell_key(source_row, col)
                    ))
                })?;
            after_formulas.push(FormulaTextAtCell {
                row: destination_row,
                col,
                formula: translated,
            });
        }
    }

    Ok(ResolvedSort {
        sheet_index,
        range,
        permutation,
        inverse_permutation,
        before_formulas,
        after_formulas,
    })
}

pub(crate) fn current_region(
    sheet: &DocumentSheet,
    anchor_row: usize,
    anchor_col: usize,
) -> Result<CellRange, AppError> {
    if matches!(cell_at(sheet, anchor_row, anchor_col), CellValue::Null) {
        return Err(AppError::DocumentStateInvalid(
            "select a non-empty cell inside the data region".to_string(),
        ));
    }

    let extent = sheet.extent();
    let occupancy = RegionOccupancy::new(sheet, extent.row_count);
    let mut range = CellRange {
        start_row: anchor_row,
        end_row: anchor_row,
        start_col: anchor_col,
        end_col: anchor_col,
    };
    loop {
        let mut changed = false;
        if range.start_row > 0
            && occupancy.row_has_value(range.start_row - 1, range.start_col, range.end_col)
        {
            range.start_row -= 1;
            changed = true;
        }
        if range.end_row + 1 < extent.row_count
            && occupancy.row_has_value(range.end_row + 1, range.start_col, range.end_col)
        {
            range.end_row += 1;
            changed = true;
        }
        if range.start_col > 0
            && occupancy.column_has_value(range.start_col - 1, range.start_row, range.end_row)
        {
            range.start_col -= 1;
            changed = true;
        }
        if range.end_col + 1 < extent.column_count
            && occupancy.column_has_value(range.end_col + 1, range.start_row, range.end_row)
        {
            range.end_col += 1;
            changed = true;
        }
        if !changed {
            break;
        }
    }
    if range.body_row_count() == 0 {
        return Err(AppError::DocumentStateInvalid(
            "the current data region contains a header but no data rows".to_string(),
        ));
    }
    Ok(range)
}

fn validate_sort_region(sheet: &DocumentSheet, range: CellRange) -> Result<(), AppError> {
    if sheet.merges.iter().any(|merge| {
        rectangles_intersect(
            range,
            merge.start_row as usize,
            merge.end_row as usize,
            merge.start_col as usize,
            merge.end_col as usize,
        )
    }) {
        return Err(AppError::DocumentStateInvalid(
            "sorting a region containing merged cells is not supported".to_string(),
        ));
    }
    if sheet.rich.drawings.iter().any(|drawing| {
        rectangles_intersect(
            range,
            drawing.from_row as usize,
            drawing.to_row.unwrap_or(drawing.from_row) as usize,
            drawing.from_col as usize,
            drawing.to_col.unwrap_or(drawing.from_col) as usize,
        )
    }) || sheet.rich.images.iter().any(|image| {
        let (from, to) = match &image.anchor {
            ImageAnchor::OneCell { from, .. } => (from, from),
            ImageAnchor::TwoCell { from, to } => (from, to),
        };
        rectangles_intersect(
            range,
            from.row as usize,
            to.row as usize,
            from.col as usize,
            to.col as usize,
        )
    }) {
        return Err(AppError::DocumentStateInvalid(
            "sorting a region containing drawings or images is not supported".to_string(),
        ));
    }
    Ok(())
}

fn rectangles_intersect(
    range: CellRange,
    start_row: usize,
    end_row: usize,
    start_col: usize,
    end_col: usize,
) -> bool {
    range.start_row <= end_row
        && start_row <= range.end_row
        && range.start_col <= end_col
        && start_col <= range.end_col
}

struct RegionOccupancy {
    rows: Vec<Vec<usize>>,
    columns: HashMap<usize, Vec<usize>>,
}

impl RegionOccupancy {
    fn new(sheet: &DocumentSheet, row_count: usize) -> Self {
        let mut rows = Vec::with_capacity(row_count);
        let mut columns = HashMap::<usize, Vec<usize>>::new();
        for (row_index, row) in sheet.rows.iter().enumerate() {
            let occupied = row
                .iter()
                .enumerate()
                .filter_map(|(col, value)| (!matches!(value, CellValue::Null)).then_some(col))
                .collect::<Vec<_>>();
            for col in &occupied {
                columns.entry(*col).or_default().push(row_index);
            }
            rows.push(occupied);
        }
        rows.resize_with(row_count, Vec::new);
        Self { rows, columns }
    }

    fn row_has_value(&self, row: usize, start_col: usize, end_col: usize) -> bool {
        self.rows
            .get(row)
            .is_some_and(|columns| sorted_values_intersect(columns, start_col, end_col))
    }

    fn column_has_value(&self, col: usize, start_row: usize, end_row: usize) -> bool {
        self.columns
            .get(&col)
            .is_some_and(|rows| sorted_values_intersect(rows, start_row, end_row))
    }
}

fn sorted_values_intersect(values: &[usize], start: usize, end: usize) -> bool {
    let index = values.partition_point(|value| *value < start);
    values.get(index).is_some_and(|value| *value <= end)
}

fn cell_at(sheet: &DocumentSheet, row: usize, col: usize) -> &CellValue {
    static NULL: CellValue = CellValue::Null;
    sheet
        .rows
        .get(row)
        .and_then(|values| values.get(col))
        .unwrap_or(&NULL)
}

#[cfg(test)]
fn compare_sort_values(left: &CellValue, right: &CellValue, direction: SortDirection) -> Ordering {
    let left = comparable_value(left);
    let right = comparable_value(right);
    compare_comparable_values(&left, &right, direction)
}

fn compare_comparable_values(
    left: &ComparableValue<'_>,
    right: &ComparableValue<'_>,
    direction: SortDirection,
) -> Ordering {
    if left.is_blank() || right.is_blank() {
        return match (left.is_blank(), right.is_blank()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => unreachable!(),
        };
    }
    let rank_ordering = left.rank().cmp(&right.rank());
    if rank_ordering != Ordering::Equal {
        return rank_ordering;
    }
    let ordering = left.compare_same_kind(right);
    match direction {
        SortDirection::Ascending => ordering,
        SortDirection::Descending => ordering.reverse(),
    }
}

enum ComparableValue<'a> {
    Number(f64),
    Text { folded: String, raw: &'a str },
    Boolean(bool),
    Error(&'a str),
    Blank,
}

impl ComparableValue<'_> {
    fn is_blank(&self) -> bool {
        matches!(self, Self::Blank)
    }

    fn rank(&self) -> u8 {
        match self {
            Self::Number(_) => 0,
            Self::Text { .. } => 1,
            Self::Boolean(_) => 2,
            Self::Error(_) => 3,
            Self::Blank => 4,
        }
    }

    fn compare_same_kind(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Number(left), Self::Number(right)) => left.total_cmp(right),
            (
                Self::Text {
                    folded: left_folded,
                    raw: left_raw,
                },
                Self::Text {
                    folded: right_folded,
                    raw: right_raw,
                },
            ) => left_folded
                .cmp(right_folded)
                .then_with(|| left_raw.cmp(right_raw)),
            (Self::Boolean(left), Self::Boolean(right)) => left.cmp(right),
            (Self::Error(left), Self::Error(right)) => left.cmp(right),
            _ => Ordering::Equal,
        }
    }
}

fn comparable_value(value: &CellValue) -> ComparableValue<'_> {
    match value {
        CellValue::Null => ComparableValue::Blank,
        CellValue::String(value) => ComparableValue::Text {
            folded: value.to_lowercase(),
            raw: value,
        },
        CellValue::Number(value) => ComparableValue::Number(value.as_f64()),
        CellValue::Boolean(value) => ComparableValue::Boolean(*value),
        CellValue::Formula {
            cached_value,
            error,
            ..
        } => error
            .as_deref()
            .map_or_else(|| comparable_value(cached_value), ComparableValue::Error),
    }
}

fn signed_delta(destination: usize, source: usize) -> Result<isize, AppError> {
    let destination = isize::try_from(destination).map_err(|_| {
        AppError::DocumentStateInvalid("sort row index exceeds supported range".to_string())
    })?;
    let source = isize::try_from(source).map_err(|_| {
        AppError::DocumentStateInvalid("sort row index exceeds supported range".to_string())
    })?;
    destination
        .checked_sub(source)
        .ok_or_else(|| AppError::DocumentStateInvalid("sort row offset overflowed".to_string()))
}

fn excel_cell_key(row: usize, col: usize) -> String {
    let mut value = col.saturating_add(1);
    let mut letters = Vec::new();
    while value > 0 {
        let remainder = (value - 1) % 26;
        letters.push((b'A' + remainder as u8) as char);
        value = (value - 1) / 26;
    }
    letters.reverse();
    format!(
        "{}{}",
        letters.into_iter().collect::<String>(),
        row.saturating_add(1)
    )
}

pub(crate) fn apply_sort_to_projection(
    sheet: &mut DocumentSheet,
    range: CellRange,
    permutation: &[usize],
    formulas: &[FormulaTextAtCell],
) {
    apply_value_permutation(sheet, range, permutation);
    apply_rich_map_permutation(&mut sheet.rich.cell_formats, range, permutation);
    apply_rich_map_permutation(&mut sheet.rich.cell_styles, range, permutation);
    apply_rich_map_permutation(&mut sheet.rich.hyperlinks, range, permutation);
    for formula in formulas {
        if let Some(CellValue::Formula {
            formula: current, ..
        }) = sheet
            .rows
            .get_mut(formula.row)
            .and_then(|row| row.get_mut(formula.col))
        {
            current.clone_from(&formula.formula);
        }
    }
}

fn apply_value_permutation(sheet: &mut DocumentSheet, range: CellRange, permutation: &[usize]) {
    let body_start = range.body_start_row();
    let mut visited = vec![false; permutation.len()];
    for start in 0..permutation.len() {
        if visited[start] || permutation[start] == start {
            visited[start] = true;
            continue;
        }
        let saved = row_segment(sheet, body_start + start, range.start_col, range.end_col);
        let mut destination = start;
        loop {
            visited[destination] = true;
            let source = permutation[destination];
            if source == start {
                write_row_segment(sheet, body_start + destination, range.start_col, &saved);
                break;
            }
            let values = row_segment(sheet, body_start + source, range.start_col, range.end_col);
            write_row_segment(sheet, body_start + destination, range.start_col, &values);
            destination = source;
        }
    }
}

fn row_segment(
    sheet: &DocumentSheet,
    row: usize,
    start_col: usize,
    end_col: usize,
) -> Vec<CellValue> {
    (start_col..=end_col)
        .map(|col| cell_at(sheet, row, col).clone())
        .collect()
}

fn write_row_segment(
    sheet: &mut DocumentSheet,
    row: usize,
    start_col: usize,
    values: &[CellValue],
) {
    if sheet.rows.len() <= row {
        sheet.rows.resize_with(row + 1, Vec::new);
    }
    let target = &mut sheet.rows[row];
    if target.len() < start_col + values.len() {
        target.resize(start_col + values.len(), CellValue::Null);
    }
    target[start_col..start_col + values.len()].clone_from_slice(values);
}

fn apply_rich_map_permutation<T: Clone>(
    map: &mut HashMap<String, T>,
    range: CellRange,
    permutation: &[usize],
) {
    let body_start = range.body_start_row();
    let mut source = HashMap::new();
    for row_offset in 0..permutation.len() {
        for col in range.start_col..=range.end_col {
            let key = excel_cell_key(body_start + row_offset, col);
            if let Some(value) = map.get(&key).cloned() {
                source.insert((row_offset, col), value);
            }
        }
    }
    for (row_offset, source_row_offset) in permutation.iter().copied().enumerate() {
        for col in range.start_col..=range.end_col {
            let key = excel_cell_key(body_start + row_offset, col);
            map.remove(&key);
            if let Some(value) = source.get(&(source_row_offset, col)) {
                map.insert(key, value.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CellNumber, parse_cell_text};

    fn sheet(rows: &[&[&str]]) -> DocumentSheet {
        DocumentSheet {
            rows: rows
                .iter()
                .map(|row| row.iter().map(|value| parse_cell_text(value)).collect())
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn current_region_expands_until_blank_boundaries() {
        let sheet = sheet(&[
            &["outside", "", ""],
            &["", "Name", "Age"],
            &["", "Ada", "37"],
            &["", "Bob", "42"],
        ]);
        assert_eq!(
            current_region(&sheet, 2, 1).unwrap(),
            CellRange {
                start_row: 1,
                end_row: 3,
                start_col: 1,
                end_col: 2,
            }
        );
    }

    #[test]
    fn text_sort_uses_unicode_lowercase_then_raw_text() {
        let mut sheet = sheet(&[&["Name"], &["b"], &["A"], &["a"], &[""]]);
        let sort = resolve_sort(0, &sheet, 0, 0, SortDirection::Ascending).unwrap();
        apply_sort_to_projection(
            &mut sheet,
            sort.range,
            &sort.permutation,
            &sort.after_formulas,
        );
        assert_eq!(sheet.rows[1][0], CellValue::String("A".to_string()));
        assert_eq!(sheet.rows[2][0], CellValue::String("a".to_string()));
        assert_eq!(sheet.rows[3][0], CellValue::String("b".to_string()));
        assert_eq!(sheet.rows[4][0], CellValue::Null);
    }

    #[test]
    fn blanks_remain_last_when_sorting_descending() {
        let left = CellValue::Null;
        let right = CellValue::Number(CellNumber::from(1));
        assert_eq!(
            compare_sort_values(&left, &right, SortDirection::Descending),
            Ordering::Greater
        );
    }

    #[test]
    fn sort_is_not_limited_by_region_query_batch_size() {
        let mut rows = Vec::with_capacity(2_049);
        rows.push(vec![CellValue::String("Value".to_string())]);
        rows.extend(
            (0..2_048)
                .rev()
                .map(|value| vec![CellValue::Number(CellNumber::from(value))]),
        );
        let sheet = DocumentSheet {
            rows,
            ..Default::default()
        };

        let sort = resolve_sort(0, &sheet, 0, 0, SortDirection::Ascending).unwrap();

        assert_eq!(sort.range.body_row_count(), 2_048);
        assert_eq!(sort.permutation.first(), Some(&2_047));
        assert_eq!(sort.permutation.last(), Some(&0));
    }
}
