use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::capabilities::WorkbookCapabilities;
use super::formula::FormulaStatus;

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct EditorStateInfo {
    pub can_undo: bool,
    pub can_redo: bool,
    pub is_dirty: bool,
    #[serde(default)]
    pub history: HistoryStatus,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct HistoryStatus {
    pub is_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
    pub undo_entries: usize,
    pub redo_entries: usize,
    pub undo_estimated_bytes: usize,
    pub redo_estimated_bytes: usize,
    pub max_history_bytes: usize,
    pub max_single_entry_bytes: usize,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct EditorSessionInfo {
    #[serde(with = "crate::types::u64_string")]
    #[ts(type = "U64String")]
    pub document_id: u64,
    #[serde(with = "crate::types::u64_string")]
    #[ts(type = "U64String")]
    pub revision: u64,
    pub formula_status: FormulaStatus,
    pub capabilities: WorkbookCapabilities,
    pub editor_state: EditorStateInfo,
}
