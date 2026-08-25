use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CellFormatProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_id: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetCellChange {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub display_text: String,
    pub edit_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula_error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetExtent {
    pub row_count: usize,
    pub column_count: usize,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetLayoutProjection {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub column_widths: HashMap<usize, u32>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub row_heights: HashMap<usize, u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetManifest {
    pub name: String,
    pub extent: SheetExtent,
    pub layout: SheetLayoutProjection,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DocumentManifest {
    pub path: String,
    pub file_name: String,
    pub sheets: Vec<SheetManifest>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MergeRange {
    pub start_row: u32,
    pub start_col: u16,
    pub end_row: u32,
    pub end_col: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetRegion {
    pub sheet_index: usize,
    pub row_start: usize,
    pub row_end: usize,
    pub col_start: usize,
    pub col_end: usize,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetRegionMetadata {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merges: Vec<MergeRange>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub cell_formats: HashMap<String, CellFormatProjection>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub cell_styles: HashMap<String, CellStyleProjection>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetRegionProjectionResponse {
    #[serde(with = "crate::u64_string")]
    pub document_id: u64,
    #[serde(with = "crate::u64_string")]
    pub revision: u64,
    pub region: SheetRegion,
    pub cells: Vec<SheetCellChange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merge_anchor_cells: Vec<SheetCellChange>,
    pub metadata: SheetRegionMetadata,
    pub wire_bytes: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetRowsRegionProjectionResponse {
    pub regions: Vec<SheetRegionProjectionResponse>,
    pub wire_bytes: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
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

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkbookImageCapabilities {
    #[serde(default)]
    pub can_insert: bool,
    #[serde(default)]
    pub can_move_resize: bool,
    #[serde(default)]
    pub can_delete: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkbookRichCapabilities {
    #[serde(default)]
    pub can_edit_styles: bool,
    #[serde(default)]
    pub can_edit_drawings: bool,
    #[serde(default)]
    pub can_edit_hyperlinks: bool,
    pub images: WorkbookImageCapabilities,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
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

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkbookCapabilities {
    pub save: WorkbookSaveCapabilities,
    pub structure: WorkbookStructureCapabilities,
    pub rich: WorkbookRichCapabilities,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sheets: Vec<SheetCapabilities>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EditorStateInfo {
    pub can_undo: bool,
    pub can_redo: bool,
    pub is_dirty: bool,
    #[serde(default)]
    pub history: HistoryStatus,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CellRangeInfo {
    pub start_row: usize,
    pub end_row: usize,
    pub start_col: usize,
    pub end_col: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FilterOperatorInfo {
    Equals,
    NotEquals,
    Contains,
    Blank,
    NotBlank,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FilterConditionInfo {
    pub col: usize,
    pub operator: FilterOperatorInfo,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetFilterInfo {
    pub sheet_index: usize,
    pub range: CellRangeInfo,
    pub conditions: Vec<FilterConditionInfo>,
    pub hidden_rows: Vec<usize>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EditorSessionInfo {
    #[serde(with = "crate::u64_string")]
    pub document_id: u64,
    #[serde(with = "crate::u64_string")]
    pub revision: u64,
    pub formula_status: FormulaStatus,
    pub capabilities: WorkbookCapabilities,
    pub editor_state: EditorStateInfo,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<SheetFilterInfo>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FormulaDiagnostics {
    pub invalid_formula_count: usize,
    pub volatile_formula_count: usize,
    pub unsupported_dependency_count: usize,
    pub large_range_dependency_count: usize,
    pub skipped_reference_rewrite_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<FormulaIssue>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FormulaIssueKind {
    InvalidFormula,
    VolatileFormula,
    UnsupportedDependency,
    LargeRangeDependency,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FormulaIssue {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub kind: FormulaIssueKind,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum FormulaStatus {
    Ready {
        #[serde(default)]
        diagnostics: FormulaDiagnostics,
    },
    Degraded {
        message: String,
        #[serde(default)]
        diagnostics: FormulaDiagnostics,
    },
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImageMarker {
    pub row: u32,
    pub col: u32,
    pub row_offset_emu: i32,
    pub col_offset_emu: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", content = "data")]
pub enum ImageAnchor {
    OneCell {
        from: ImageMarker,
        #[serde(rename = "widthEmu")]
        width_emu: u32,
        #[serde(rename = "heightEmu")]
        height_emu: u32,
    },
    TwoCell {
        from: ImageMarker,
        to: ImageMarker,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetImage {
    pub id: String,
    pub media_id: String,
    pub mime_type: String,
    pub intrinsic_width: u32,
    pub intrinsic_height: u32,
    pub anchor: ImageAnchor,
    pub z_index: usize,
    pub renderable: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenDocumentResponse {
    pub document: DocumentManifest,
    pub editor_session: EditorSessionInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_region: Option<SheetRegionProjectionResponse>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SavedDocumentIdentity {
    pub path: String,
    pub file_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SavedDocumentResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<DocumentManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<SavedDocumentIdentity>,
    pub editor_session: EditorSessionInfo,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LayoutPatch {
    pub sheet_index: usize,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub column_widths: HashMap<usize, Option<u32>>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub row_heights: HashMap<usize, Option<u32>>,
}

macro_rules! sheet_patch {
    ($name:ident) => {
        #[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            pub sheet_index: usize,
        }
    };
}

sheet_patch!(SheetDeletedPatch);
sheet_patch!(SheetInvalidatedPatch);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetInsertedPatch {
    pub sheet_index: usize,
    pub sheet: SheetManifest,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetsReplacedPatch {
    pub start_index: usize,
    pub sheets: Vec<SheetManifest>,
}

macro_rules! indexed_patch {
    ($name:ident, $field:ident) => {
        #[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            pub sheet_index: usize,
            pub $field: usize,
            pub count: usize,
        }
    };
}

indexed_patch!(RowInsertedPatch, row_index);
indexed_patch!(RowDeletedPatch, row_index);
indexed_patch!(ColumnInsertedPatch, col_index);
indexed_patch!(ColumnDeletedPatch, col_index);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResyncRequiredPatch {
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImageUpsertedPatch {
    pub sheet_index: usize,
    pub image: SheetImage,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImageDeletedPatch {
    pub sheet_index: usize,
    pub image_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", content = "data")]
pub enum EditorPatch {
    Cells { changes: Vec<SheetCellChange> },
    Layout { patch: LayoutPatch },
    SheetInserted { patch: SheetInsertedPatch },
    SheetDeleted { patch: SheetDeletedPatch },
    SheetInvalidated { patch: SheetInvalidatedPatch },
    SheetsReplaced { patch: SheetsReplacedPatch },
    RowInserted { patch: RowInsertedPatch },
    RowDeleted { patch: RowDeletedPatch },
    ColumnInserted { patch: ColumnInsertedPatch },
    ColumnDeleted { patch: ColumnDeletedPatch },
    ImageUpserted { patch: ImageUpsertedPatch },
    ImageDeleted { patch: ImageDeletedPatch },
    ResyncRequired { patch: ResyncRequiredPatch },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EditorMutationResponse {
    #[serde(with = "crate::u64_string")]
    pub document_id: u64,
    #[serde(with = "crate::u64_string")]
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub sheet_index: usize,
    pub sheet_name: String,
    pub row: usize,
    pub col: usize,
    pub value: String,
    pub cell_position: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub truncated: bool,
}
