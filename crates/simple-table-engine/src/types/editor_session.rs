use serde::{Deserialize, Serialize};

use super::capabilities::WorkbookCapabilities;
use super::formula::FormulaStatus;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EditorStateInfo {
    pub can_undo: bool,
    pub can_redo: bool,
    pub is_dirty: bool,
    #[serde(default)]
    pub history: HistoryStatus,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct HistoryStatus {
    pub is_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub undo_entries: usize,
    pub redo_entries: usize,
    pub undo_estimated_bytes: usize,
    pub redo_estimated_bytes: usize,
    pub max_history_bytes: usize,
    pub max_single_entry_bytes: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EditorSessionInfo {
    #[serde(with = "crate::types::u64_string")]
    pub document_id: u64,
    #[serde(with = "crate::types::u64_string")]
    pub revision: u64,
    pub formula_status: FormulaStatus,
    pub capabilities: WorkbookCapabilities,
    pub editor_state: EditorStateInfo,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<SheetFilterInfo>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CellRangeInfo {
    pub start_row: usize,
    pub end_row: usize,
    pub start_col: usize,
    pub end_col: usize,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FilterOperatorInfo {
    Equals,
    NotEquals,
    Contains,
    Blank,
    NotBlank,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FilterConditionInfo {
    pub col: usize,
    pub operator: FilterOperatorInfo,
    pub value: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetFilterInfo {
    pub sheet_index: usize,
    pub range: CellRangeInfo,
    pub conditions: Vec<FilterConditionInfo>,
    pub hidden_rows: Vec<usize>,
}
