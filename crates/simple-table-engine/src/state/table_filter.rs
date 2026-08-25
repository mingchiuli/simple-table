use std::collections::BTreeMap;

use crate::document_data::{DocumentData, DocumentSheet};
use crate::domain::{AppliedOperation, CellRange, CellValue, FilterOperator, current_region};
use crate::error::AppError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterCondition {
    pub col: usize,
    pub operator: FilterOperator,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetFilterState {
    pub sheet_index: usize,
    pub range: CellRange,
    pub conditions: Vec<FilterCondition>,
    pub hidden_rows: Vec<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TableFilterState {
    sheets: BTreeMap<usize, SheetFilterState>,
}

impl TableFilterState {
    pub(crate) fn snapshots(&self) -> Vec<SheetFilterState> {
        self.sheets.values().cloned().collect()
    }

    pub(crate) fn history_snapshot(&self) -> Self {
        let mut snapshot = self.clone();
        for state in snapshot.sheets.values_mut() {
            state.hidden_rows.clear();
        }
        snapshot
    }

    pub(crate) fn set_condition(
        &mut self,
        file_data: &DocumentData,
        sheet_index: usize,
        anchor_row: usize,
        col: usize,
        operator: FilterOperator,
        value: String,
    ) -> Result<(), AppError> {
        if value.len() > crate::resource_limits::MAX_CELL_TEXT_BYTES {
            return Err(AppError::ResourceLimitExceeded(format!(
                "filter value requires {} bytes; the maximum is {} bytes",
                value.len(),
                crate::resource_limits::MAX_CELL_TEXT_BYTES
            )));
        }
        let value = if matches!(operator, FilterOperator::Blank | FilterOperator::NotBlank) {
            String::new()
        } else {
            value
        };
        let sheet = file_data
            .sheets
            .get(sheet_index)
            .ok_or(AppError::InvalidSheetIndex(sheet_index))?;
        let range = current_region(sheet, anchor_row, col)?;
        if sheet.merges.iter().any(|merge| {
            range.start_row <= merge.end_row as usize
                && merge.start_row as usize <= range.end_row
                && range.start_col <= merge.end_col as usize
                && merge.start_col as usize <= range.end_col
        }) {
            return Err(AppError::DocumentStateInvalid(
                "filtering a region containing merged cells is not supported".to_string(),
            ));
        }
        if !(range.start_col..=range.end_col).contains(&col) {
            return Err(AppError::DocumentStateInvalid(
                "the filter column must be inside the current data region".to_string(),
            ));
        }
        let state = self.sheets.entry(sheet_index).or_insert(SheetFilterState {
            sheet_index,
            range,
            conditions: Vec::new(),
            hidden_rows: Vec::new(),
        });
        if state.range != range {
            state.range = range;
            state.conditions.clear();
        }
        state.conditions.retain(|condition| condition.col != col);
        state.conditions.push(FilterCondition {
            col,
            operator,
            value,
        });
        state.conditions.sort_by_key(|condition| condition.col);
        recompute_sheet_filter(state, sheet);
        Ok(())
    }

    pub(crate) fn clear(&mut self, sheet_index: usize, col: Option<usize>) {
        let Some(col) = col else {
            self.sheets.remove(&sheet_index);
            return;
        };
        if let Some(state) = self.sheets.get_mut(&sheet_index) {
            state.conditions.retain(|condition| condition.col != col);
            if state.conditions.is_empty() {
                self.sheets.remove(&sheet_index);
            }
        }
    }

    pub(crate) fn after_operation(
        &mut self,
        file_data: &DocumentData,
        operation: &AppliedOperation,
    ) {
        self.remap_for_operation(operation);
        self.recompute(file_data);
    }

    pub(crate) fn recompute(&mut self, file_data: &DocumentData) {
        self.sheets.retain(|sheet_index, state| {
            let Some(sheet) = file_data.sheets.get(*sheet_index) else {
                return false;
            };
            recompute_sheet_filter(state, sheet);
            true
        });
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self
                .sheets
                .values()
                .map(|state| {
                    std::mem::size_of::<SheetFilterState>()
                        + state.hidden_rows.len() * std::mem::size_of::<usize>()
                        + state
                            .conditions
                            .iter()
                            .map(|condition| {
                                std::mem::size_of::<FilterCondition>() + condition.value.len()
                            })
                            .sum::<usize>()
                })
                .sum::<usize>()
    }

    fn remap_for_operation(&mut self, operation: &AppliedOperation) {
        match operation {
            AppliedOperation::AddRow {
                sheet_index,
                row_index,
                ..
            } => {
                if let Some(state) = self.sheets.get_mut(sheet_index) {
                    if *row_index <= state.range.start_row {
                        state.range.start_row += 1;
                        state.range.end_row += 1;
                    } else if *row_index <= state.range.end_row {
                        state.range.end_row += 1;
                    }
                }
            }
            AppliedOperation::DeleteRow {
                sheet_index,
                row_index,
            } => {
                let remove = self.sheets.get(sheet_index).is_some_and(|state| {
                    *row_index == state.range.start_row
                        || state.range.start_row == state.range.end_row
                });
                if remove {
                    self.sheets.remove(sheet_index);
                } else if let Some(state) = self.sheets.get_mut(sheet_index) {
                    if *row_index < state.range.start_row {
                        state.range.start_row -= 1;
                        state.range.end_row -= 1;
                    } else if *row_index <= state.range.end_row {
                        state.range.end_row -= 1;
                    }
                }
            }
            AppliedOperation::AddColumn {
                sheet_index,
                col_index,
                ..
            } => {
                if let Some(state) = self.sheets.get_mut(sheet_index) {
                    if *col_index <= state.range.start_col {
                        state.range.start_col += 1;
                        state.range.end_col += 1;
                    } else if *col_index <= state.range.end_col {
                        state.range.end_col += 1;
                    }
                    for condition in &mut state.conditions {
                        if condition.col >= *col_index {
                            condition.col += 1;
                        }
                    }
                }
            }
            AppliedOperation::DeleteColumn {
                sheet_index,
                col_index,
            } => {
                if let Some(state) = self.sheets.get_mut(sheet_index) {
                    state
                        .conditions
                        .retain(|condition| condition.col != *col_index);
                    for condition in &mut state.conditions {
                        if condition.col > *col_index {
                            condition.col -= 1;
                        }
                    }
                    if *col_index < state.range.start_col {
                        state.range.start_col -= 1;
                        state.range.end_col -= 1;
                    } else if *col_index <= state.range.end_col {
                        state.range.end_col = state.range.end_col.saturating_sub(1);
                    }
                }
                if self
                    .sheets
                    .get(sheet_index)
                    .is_some_and(|state| state.conditions.is_empty())
                {
                    self.sheets.remove(sheet_index);
                }
            }
            AppliedOperation::AddSheet { sheet_index, .. } => {
                self.shift_sheet_indexes(*sheet_index, true);
            }
            AppliedOperation::DeleteSheet { sheet_index } => {
                self.sheets.remove(sheet_index);
                self.shift_sheet_indexes(*sheet_index, false);
            }
            AppliedOperation::SetCell { .. }
            | AppliedOperation::SetCells { .. }
            | AppliedOperation::SetColumnWidth { .. }
            | AppliedOperation::SetRowHeight { .. }
            | AppliedOperation::SortRows(_)
            | AppliedOperation::InsertImage { .. }
            | AppliedOperation::UpdateImage { .. }
            | AppliedOperation::DeleteImage { .. } => {}
        }
    }

    fn shift_sheet_indexes(&mut self, index: usize, insert: bool) {
        let old = std::mem::take(&mut self.sheets);
        self.sheets = old
            .into_values()
            .map(|mut state| {
                if insert && state.sheet_index >= index {
                    state.sheet_index += 1;
                } else if !insert && state.sheet_index > index {
                    state.sheet_index -= 1;
                }
                (state.sheet_index, state)
            })
            .collect();
    }
}

fn recompute_sheet_filter(state: &mut SheetFilterState, sheet: &DocumentSheet) {
    let prepared = state
        .conditions
        .iter()
        .map(|condition| (condition, condition.value.to_lowercase()))
        .collect::<Vec<_>>();
    state.hidden_rows = (state.range.body_start_row()..=state.range.end_row)
        .filter(|row| {
            !prepared.iter().all(|(condition, folded)| {
                matches_condition(cell_at(sheet, *row, condition.col), condition, folded)
            })
        })
        .collect();
}

fn matches_condition(value: &CellValue, condition: &FilterCondition, folded: &str) -> bool {
    let blank = is_blank(value);
    match condition.operator {
        FilterOperator::Blank => blank,
        FilterOperator::NotBlank => !blank,
        FilterOperator::Equals => value.to_display_string().to_lowercase() == folded,
        FilterOperator::NotEquals => value.to_display_string().to_lowercase() != folded,
        FilterOperator::Contains => value.to_display_string().to_lowercase().contains(folded),
    }
}

fn is_blank(value: &CellValue) -> bool {
    match value {
        CellValue::Null => true,
        CellValue::Formula {
            cached_value,
            error,
            ..
        } => error.is_none() && is_blank(cached_value),
        _ => false,
    }
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
mod tests {
    use super::*;
    use crate::domain::parse_cell_text;

    #[test]
    fn conditions_are_combined_with_and() {
        let data = DocumentData {
            path: String::new(),
            file_name: "filter.xlsx".to_string(),
            sheets: vec![DocumentSheet {
                rows: vec![
                    vec![parse_cell_text("Name"), parse_cell_text("Team")],
                    vec![parse_cell_text("Ada"), parse_cell_text("Core")],
                    vec![parse_cell_text("Alan"), parse_cell_text("UI")],
                    vec![parse_cell_text("Bob"), parse_cell_text("Core")],
                ],
                ..Default::default()
            }],
        };
        let mut filters = TableFilterState::default();
        filters
            .set_condition(&data, 0, 0, 0, FilterOperator::Contains, "a".into())
            .unwrap();
        filters
            .set_condition(&data, 0, 0, 1, FilterOperator::Equals, "core".into())
            .unwrap();
        assert_eq!(filters.snapshots()[0].hidden_rows, vec![2, 3]);
    }
}
