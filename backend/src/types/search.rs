use serde::{Deserialize, Serialize};

/// 搜索结果
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub sheet_index: usize,
    pub sheet_name: String,
    pub row: usize,
    pub col: usize,
    pub value: String,
    pub cell_position: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub truncated: bool,
}

/// 搜索范围
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub enum SearchScope {
    CurrentSheet,
    AllSheets,
}
