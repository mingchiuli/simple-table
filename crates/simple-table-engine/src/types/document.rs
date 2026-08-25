use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::cell::CellFormatProjection;
use super::cell_change::{CellStyleProjection, SheetCellChange};
use super::editor_session::EditorSessionInfo;

/// 合并范围
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MergeRange {
    pub start_row: u32,
    pub start_col: u16,
    pub end_row: u32,
    pub end_col: u16,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetExtent {
    pub row_count: usize,
    pub column_count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetManifest {
    pub name: String,
    pub extent: SheetExtent,
    pub layout: SheetLayoutProjection,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetLayoutProjection {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub column_widths: HashMap<usize, u32>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub row_heights: HashMap<usize, u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DocumentManifest {
    pub path: String,
    pub file_name: String,
    pub sheets: Vec<SheetManifest>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OpenDocumentResponse {
    pub document: DocumentManifest,
    pub editor_session: EditorSessionInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_region: Option<SheetRegionProjectionResponse>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetRegion {
    pub sheet_index: usize,
    pub row_start: usize,
    pub row_end: usize,
    pub col_start: usize,
    pub col_end: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SheetRegionMetadata {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merges: Vec<MergeRange>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub cell_formats: HashMap<String, CellFormatProjection>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub cell_styles: HashMap<String, CellStyleProjection>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SheetRegionProjectionResponse {
    #[serde(with = "crate::types::u64_string")]
    pub document_id: u64,
    #[serde(with = "crate::types::u64_string")]
    pub revision: u64,
    pub region: SheetRegion,
    pub cells: Vec<SheetCellChange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merge_anchor_cells: Vec<SheetCellChange>,
    pub metadata: SheetRegionMetadata,
    pub wire_bytes: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SheetRowsRegionProjectionResponse {
    pub regions: Vec<SheetRegionProjectionResponse>,
    pub wire_bytes: usize,
}
