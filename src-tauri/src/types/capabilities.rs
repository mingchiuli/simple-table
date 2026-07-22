use serde::{Deserialize, Serialize};
use ts_rs::TS;

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
