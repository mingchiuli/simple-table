use crate::domain::CellValue as DomainCellValue;
use crate::types::cell::{CellFormatProjection, CellValue, CellValueProjection};
use crate::types::{EditorSessionInfo, EditorStateInfo, FormulaStatus};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ts_rs::TS;

#[cfg(any(target_os = "android", target_os = "ios"))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedFileInfo {
    pub path: String,
    pub original_path: String,
    pub file_name: String,
}

#[cfg(desktop)]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopOpenFileInfo {
    pub path: String,
    pub file_name: String,
}

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

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PreparedOpenDocument {
    pub token: String,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SavedDocumentIdentity {
    pub path: String,
    pub file_name: String,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SavedDocumentResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub document: Option<DocumentManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub identity: Option<SavedDocumentIdentity>,
    pub editor_session: EditorSessionInfo,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SpreadsheetFormatOptions {
    pub default_extension: String,
    pub supported_extensions: Vec<String>,
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

#[derive(Serialize, Deserialize, TS, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CellStyleProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizontal_align: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical_align: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number_format: Option<String>,
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

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct DocumentCapabilities {
    #[ts(type = "\"xlsx\" | \"csv\"")]
    pub source_format: String,
    pub can_save_original: bool,
    #[ts(type = "\"xlsx\" | \"csv\" | null")]
    pub native_save_format: Option<String>,
    #[ts(type = "Array<\"xlsx\" | \"csv\">")]
    pub export_formats: Vec<String>,
    #[ts(type = "\"xlsx\" | \"csv\" | null")]
    pub native_save_extension: Option<String>,
    #[ts(type = "\"xlsx\" | \"csv\"")]
    pub export_extension: String,
    pub requires_save_as_for_native_save: bool,
    #[serde(default)]
    pub workbook: WorkbookCapabilities,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct NativeSavePlan {
    pub can_save: bool,
    pub requires_save_as: bool,
    #[ts(type = "\"xlsx\" | \"csv\" | null")]
    pub native_save_extension: Option<String>,
    #[ts(type = "\"xlsx\" | \"csv\"")]
    pub default_extension: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    pub capabilities: DocumentCapabilities,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SheetCapabilities {
    pub can_edit_cells: bool,
    pub can_resize_rows_columns: bool,
    pub can_insert_delete_rows: bool,
    pub can_insert_delete_columns: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_edit_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_resize_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_row_structure_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_column_structure_reasons: Vec<String>,
}

impl Default for SheetCapabilities {
    fn default() -> Self {
        Self {
            can_edit_cells: true,
            can_resize_rows_columns: true,
            can_insert_delete_rows: true,
            can_insert_delete_columns: true,
            blocked_edit_reasons: Vec::new(),
            blocked_resize_reasons: Vec::new(),
            blocked_row_structure_reasons: Vec::new(),
            blocked_column_structure_reasons: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct WorkbookSaveCapabilities {
    pub can_native_save: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_save_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detected_features: Vec<String>,
}

impl Default for WorkbookSaveCapabilities {
    fn default() -> Self {
        Self {
            can_native_save: true,
            blocked_save_reasons: Vec::new(),
            detected_features: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct WorkbookStructureCapabilities {
    #[serde(default)]
    pub can_insert_delete_sheets: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_structure_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_sheet_structure_reasons: Vec<String>,
}

impl Default for WorkbookStructureCapabilities {
    fn default() -> Self {
        Self {
            can_insert_delete_sheets: true,
            blocked_structure_reasons: Vec::new(),
            blocked_sheet_structure_reasons: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct WorkbookRichCapabilities {
    #[serde(default)]
    pub can_edit_styles: bool,
    #[serde(default)]
    pub can_edit_drawings: bool,
    #[serde(default)]
    pub can_edit_hyperlinks: bool,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct WorkbookCapabilities {
    pub save: WorkbookSaveCapabilities,
    pub structure: WorkbookStructureCapabilities,
    pub rich: WorkbookRichCapabilities,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sheets: Vec<SheetCapabilities>,
}

/// 带 sheet 的单元格变化，用于高频编辑的增量响应。
#[derive(Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SheetCellChange {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub value: CellValue,
    #[serde(default, skip)]
    #[ts(skip)]
    pub display: Option<String>,
    #[serde(default, skip)]
    #[ts(skip)]
    pub format: Option<CellFormatProjection>,
    #[serde(default, skip)]
    #[ts(skip)]
    pub style: Option<CellStyleProjection>,
    #[serde(default)]
    #[ts(skip)]
    pub display_format: Option<CellFormatProjection>,
}

impl SheetCellChange {
    pub fn new(sheet_index: usize, row: usize, col: usize, value: DomainCellValue) -> Self {
        Self {
            sheet_index,
            row,
            col,
            value: value.into(),
            display: None,
            format: None,
            style: None,
            display_format: None,
        }
    }

    pub fn with_display_projection(
        mut self,
        display: String,
        format: Option<CellFormatProjection>,
        style: Option<CellStyleProjection>,
    ) -> Self {
        self.display = Some(display);
        self.format = format.clone();
        self.style = style;
        self.display_format = format;
        self
    }
}

impl Serialize for SheetCellChange {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("SheetCellChange", 4)?;
        state.serialize_field("sheetIndex", &self.sheet_index)?;
        state.serialize_field("row", &self.row)?;
        state.serialize_field("col", &self.col)?;
        state.serialize_field(
            "value",
            &CellValueProjection::new(self.value.as_domain(), self.display_format.clone()),
        )?;
        state.end()
    }
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct LayoutPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub column_widths: HashMap<usize, Option<u32>>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub row_heights: HashMap<usize, Option<u32>>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SheetInsertedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    pub sheet: SheetManifest,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SheetDeletedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SheetInvalidatedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SheetsReplacedPatch {
    #[serde(rename = "startIndex")]
    pub start_index: usize,
    pub sheets: Vec<SheetManifest>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RowInsertedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    #[serde(rename = "rowIndex")]
    pub row_index: usize,
    pub count: usize,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RowDeletedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    #[serde(rename = "rowIndex")]
    pub row_index: usize,
    pub count: usize,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ColumnInsertedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    #[serde(rename = "colIndex")]
    pub col_index: usize,
    pub count: usize,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ColumnDeletedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    #[serde(rename = "colIndex")]
    pub col_index: usize,
    pub count: usize,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ResyncRequiredPatch {
    pub reason: String,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(tag = "type", content = "data")]
#[ts(tag = "type", content = "data")]
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
    #[serde(rename = "ResyncRequired")]
    ResyncRequired { patch: ResyncRequiredPatch },
}

#[cfg(test)]
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct EditorCommandContext {
    #[serde(with = "crate::types::u64_string")]
    #[ts(type = "U64String")]
    pub document_id: u64,
    #[serde(with = "crate::types::u64_string")]
    #[ts(type = "U64String")]
    pub base_revision: u64,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct EditorMutationResponse {
    #[ts(type = "typeof EDITOR_MUTATION_PROTOCOL_VERSION")]
    pub protocol_version: u16,
    #[serde(with = "crate::types::u64_string")]
    #[ts(type = "U64String")]
    pub document_id: u64,
    #[serde(with = "crate::types::u64_string")]
    #[ts(type = "U64String")]
    pub revision: u64,
    pub formula_status: FormulaStatus,
    #[serde(default)]
    pub capabilities: WorkbookCapabilities,
    pub editor_state: EditorStateInfo,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patches: Vec<EditorPatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sheet_extents: Option<Vec<SheetExtent>>,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum MutationResultStatus {
    Pending,
    Completed,
    Missing,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct MutationResultLookup {
    pub status: MutationResultStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub response: Option<EditorMutationResponse>,
}
