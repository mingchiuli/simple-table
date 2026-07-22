use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ts_rs::TS;

use super::cell::CellFormatProjection;
use super::cell_change::{CellStyleProjection, SheetCellChange};
use super::editor_session::EditorSessionInfo;

/// 合并范围
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct MergeRange {
    pub start_row: u32,
    pub start_col: u16,
    pub end_row: u32,
    pub end_col: u16,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SheetExtent {
    pub row_count: usize,
    pub column_count: usize,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SheetManifest {
    pub name: String,
    pub extent: SheetExtent,
    pub layout: SheetLayoutProjection,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SheetLayoutProjection {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub column_widths: HashMap<usize, u32>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub row_heights: HashMap<usize, u32>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct DocumentManifest {
    pub path: String,
    pub file_name: String,
    pub sheets: Vec<SheetManifest>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct OpenDocumentResponse {
    pub document: DocumentManifest,
    pub editor_session: EditorSessionInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub initial_region: Option<SheetRegionProjectionResponse>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SheetRegion {
    pub sheet_index: usize,
    pub row_start: usize,
    pub row_end: usize,
    pub col_start: usize,
    pub col_end: usize,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SheetRegionMetadata {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merges: Vec<MergeRange>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub cell_formats: HashMap<String, CellFormatProjection>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub cell_styles: HashMap<String, CellStyleProjection>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SheetRegionProjectionResponse {
    #[serde(with = "crate::types::u64_string")]
    #[ts(type = "U64String")]
    pub document_id: u64,
    #[serde(with = "crate::types::u64_string")]
    #[ts(type = "U64String")]
    pub revision: u64,
    pub region: SheetRegion,
    pub cells: Vec<SheetCellChange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merge_anchor_cells: Vec<SheetCellChange>,
    pub metadata: SheetRegionMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub estimated_bytes: Option<usize>,
}

#[allow(dead_code)]
#[derive(Serialize, Deserialize, TS, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "ReadOnlyRichProjection", rename_all = "camelCase")]
pub struct ReadOnlyRichProjection {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub cell_formats: HashMap<String, CellFormatProjection>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub cell_styles: HashMap<String, CellStyleProjection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_rows: Vec<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_columns: Vec<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freeze_pane: Option<FreezePaneProjection>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub hyperlinks: HashMap<String, HyperlinkProjection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drawings: Vec<DrawingProjection>,
    #[serde(default)]
    pub has_more_drawings: bool,
    #[serde(default)]
    pub has_style_metadata: bool,
    #[serde(default)]
    pub has_hyperlinks: bool,
    #[serde(default)]
    pub has_freeze_pane: bool,
}

#[allow(dead_code)]
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FreezePaneProjection {
    pub top_left_cell: String,
    pub horizontal_split: f64,
    pub vertical_split: f64,
    pub active_pane: String,
    pub state: String,
}

#[allow(dead_code)]
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct HyperlinkProjection {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
    pub location: bool,
}

#[allow(dead_code)]
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct DrawingProjection {
    pub kind: DrawingKind,
    pub from_row: u32,
    pub from_col: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_row: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_col: Option<u32>,
}

#[allow(dead_code)]
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum DrawingKind {
    Image,
    Chart,
}
