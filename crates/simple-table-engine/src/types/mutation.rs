use serde::{Deserialize, Serialize};

pub const EDITOR_MUTATION_PROTOCOL_VERSION: u16 = 5;
use std::collections::HashMap;

use super::capabilities::WorkbookCapabilities;
use super::cell_change::SheetCellChange;
use super::document::{SheetExtent, SheetManifest};
use super::editor_session::{EditorStateInfo, SheetFilterInfo};
use super::formula::FormulaStatus;
use super::image::SheetImage;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LayoutPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub column_widths: HashMap<usize, Option<u32>>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub row_heights: HashMap<usize, Option<u32>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SheetInsertedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    pub sheet: SheetManifest,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SheetDeletedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SheetInvalidatedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SheetsReplacedPatch {
    #[serde(rename = "startIndex")]
    pub start_index: usize,
    pub sheets: Vec<SheetManifest>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RowInsertedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    #[serde(rename = "rowIndex")]
    pub row_index: usize,
    pub count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RowDeletedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    #[serde(rename = "rowIndex")]
    pub row_index: usize,
    pub count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ColumnInsertedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    #[serde(rename = "colIndex")]
    pub col_index: usize,
    pub count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ColumnDeletedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    #[serde(rename = "colIndex")]
    pub col_index: usize,
    pub count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ResyncRequiredPatch {
    pub reason: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImageUpsertedPatch {
    pub sheet_index: usize,
    pub image: SheetImage,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImageDeletedPatch {
    pub sheet_index: usize,
    pub image_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", content = "data")]
pub enum EditorPatch {
    #[serde(rename = "Cells")]
    Cells { changes: Vec<SheetCellChange> },
    #[serde(rename = "Layout")]
    Layout { patch: LayoutPatch },
    #[serde(rename = "SheetInserted")]
    SheetInserted { patch: SheetInsertedPatch },
    #[serde(rename = "SheetDeleted")]
    SheetDeleted { patch: SheetDeletedPatch },
    #[serde(rename = "SheetInvalidated")]
    SheetInvalidated { patch: SheetInvalidatedPatch },
    #[serde(rename = "SheetsReplaced")]
    SheetsReplaced { patch: SheetsReplacedPatch },
    #[serde(rename = "RowInserted")]
    RowInserted { patch: RowInsertedPatch },
    #[serde(rename = "RowDeleted")]
    RowDeleted { patch: RowDeletedPatch },
    #[serde(rename = "ColumnInserted")]
    ColumnInserted { patch: ColumnInsertedPatch },
    #[serde(rename = "ColumnDeleted")]
    ColumnDeleted { patch: ColumnDeletedPatch },
    #[serde(rename = "ImageUpserted")]
    ImageUpserted { patch: ImageUpsertedPatch },
    #[serde(rename = "ImageDeleted")]
    ImageDeleted { patch: ImageDeletedPatch },
    #[serde(rename = "ResyncRequired")]
    ResyncRequired { patch: ResyncRequiredPatch },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EditorMutationResponse {
    pub protocol_version: u16,
    #[serde(with = "crate::types::u64_string")]
    pub document_id: u64,
    #[serde(with = "crate::types::u64_string")]
    pub revision: u64,
    pub formula_status: FormulaStatus,
    #[serde(default)]
    pub capabilities: WorkbookCapabilities,
    pub editor_state: EditorStateInfo,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<SheetFilterInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patches: Vec<EditorPatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet_extents: Option<Vec<SheetExtent>>,
}
