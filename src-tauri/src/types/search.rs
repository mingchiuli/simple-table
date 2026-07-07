use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 搜索结果
#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SearchResult {
    pub sheet_index: usize,
    pub sheet_name: String,
    pub row: usize,
    pub col: usize,
    pub value: String,
    pub cell_position: String,
}

/// 搜索范围
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum SearchScope {
    CurrentSheet,
    AllSheets,
}
