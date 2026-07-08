use crate::display::DisplayProjection;
use crate::state::state::{EditorSessionInfo, EditorStateInfo};
use crate::types::FormulaStatus;
use crate::types::projection::{CellValueProjection, SheetRowsProjection};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use ts_rs::TS;

// JavaScript 安全整数范围: -(2^53 - 1) 到 (2^53 - 1)
const JS_MAX_SAFE_INTEGER: i64 = 9007199254740991;
const JS_MIN_SAFE_INTEGER: i64 = -9007199254740991;

#[derive(Clone, Debug, PartialEq)]
pub enum CellValue {
    Null,
    String(String),
    Number(Value), // 使用 serde_json::Value 支持精确大整数
    Boolean(bool),
    Formula {
        formula: String,
        cached_value: Box<CellValue>,
        error: Option<String>,
    },
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CellFormatProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_id: Option<String>,
}

impl CellValue {
    /// 将单元格值转换为字符串（用于搜索索引等场景）
    /// Null 返回空字符串，其他类型返回其字符串表示
    pub fn to_display_string(&self) -> String {
        match self {
            CellValue::Null => String::new(),
            CellValue::String(s) => s.clone(),
            CellValue::Number(n) => n.to_string(),
            CellValue::Boolean(b) => b.to_string(),
            CellValue::Formula {
                cached_value,
                error,
                ..
            } => error
                .clone()
                .unwrap_or_else(|| cached_value.to_display_string()),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            CellValue::Null => "blank",
            CellValue::String(_) => "text",
            CellValue::Number(_) => "number",
            CellValue::Boolean(_) => "boolean",
            CellValue::Formula { error, .. } => {
                if error.is_some() {
                    "error"
                } else {
                    "formula"
                }
            }
        }
    }

    pub(crate) fn raw_json_value(&self) -> Value {
        match self {
            CellValue::Null => Value::Null,
            CellValue::String(value) => Value::String(value.clone()),
            CellValue::Number(value) => {
                if let Some(i) = value.as_i64()
                    && !(JS_MIN_SAFE_INTEGER..=JS_MAX_SAFE_INTEGER).contains(&i)
                {
                    return Value::String(i.to_string());
                }
                value.clone()
            }
            CellValue::Boolean(value) => Value::Bool(*value),
            CellValue::Formula { cached_value, .. } => cached_value.raw_json_value(),
        }
    }

    pub fn formula(formula: impl Into<String>, cached_value: CellValue) -> Self {
        CellValue::Formula {
            formula: normalize_formula_text(formula.into()),
            cached_value: Box::new(cached_value),
            error: None,
        }
    }

    pub fn with_formula_result(&self, cached_value: CellValue, error: Option<String>) -> Self {
        match self {
            CellValue::Formula { formula, .. } => CellValue::Formula {
                formula: formula.clone(),
                cached_value: Box::new(cached_value),
                error,
            },
            _ => self.clone(),
        }
    }
}

pub fn normalize_formula_text(formula: String) -> String {
    if formula.starts_with('=') {
        formula
    } else {
        format!("={formula}")
    }
}

pub fn parse_cell_text(text: &str) -> CellValue {
    if text.is_empty() {
        return CellValue::Null;
    }
    if text.starts_with('=') {
        return CellValue::formula(text, CellValue::Null);
    }
    if has_leading_zero(text) {
        return CellValue::String(text.to_string());
    }
    if let Ok(value) = text.parse::<i64>() {
        if (JS_MIN_SAFE_INTEGER..=JS_MAX_SAFE_INTEGER).contains(&value) {
            return CellValue::Number(Value::from(value));
        }
        return CellValue::String(text.to_string());
    }
    if let Ok(value) = text.parse::<f64>()
        && value.is_finite()
    {
        return CellValue::Number(Value::from(value));
    }
    if text.eq_ignore_ascii_case("true") {
        return CellValue::Boolean(true);
    }
    if text.eq_ignore_ascii_case("false") {
        return CellValue::Boolean(false);
    }
    CellValue::String(text.to_string())
}

fn has_leading_zero(text: &str) -> bool {
    let bytes = text.as_bytes();
    let digits = if bytes.first() == Some(&b'-') || bytes.first() == Some(&b'+') {
        &bytes[1..]
    } else {
        bytes
    };
    digits.len() > 1 && digits[0] == b'0' && digits.iter().all(|b| b.is_ascii_digit())
}

impl<'de> Deserialize<'de> for CellValue {
    fn deserialize<D>(deserializer: D) -> Result<CellValue, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // 使用 serde_json::Value 反序列化，然后转换
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::Null => Ok(CellValue::Null),
            Value::Bool(b) => Ok(CellValue::Boolean(b)),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(CellValue::Number(Value::from(i)))
                } else if let Some(f) = n.as_f64() {
                    Ok(CellValue::Number(Value::from(f)))
                } else {
                    // 解析失败，尝试作为字符串
                    Ok(CellValue::String(n.to_string()))
                }
            }
            Value::String(s) => {
                // JS 端传入 string 时表示用户明确要保留文本语义，
                // 例如邮编/编号 "007" 或超过 JS 安全整数范围的值。
                Ok(CellValue::String(s))
            }
            Value::Object(mut object) => {
                if object.get("type").and_then(Value::as_str) == Some("cell") {
                    let formula = object.remove("formula");
                    if let Some(Value::Object(mut formula)) = formula {
                        let formula_text = formula
                            .remove("formula")
                            .and_then(|value| value.as_str().map(ToOwned::to_owned))
                            .unwrap_or_default();
                        let cached_value = formula
                            .remove("cachedValue")
                            .or_else(|| formula.remove("cached_value"))
                            .or_else(|| object.remove("raw"))
                            .map(CellValue::deserialize)
                            .transpose()
                            .map_err(serde::de::Error::custom)?
                            .unwrap_or(CellValue::Null);
                        let error = formula
                            .remove("error")
                            .and_then(|value| value.as_str().map(ToOwned::to_owned));

                        return Ok(CellValue::Formula {
                            formula: normalize_formula_text(formula_text),
                            cached_value: Box::new(cached_value),
                            error,
                        });
                    }

                    return object
                        .remove("raw")
                        .map(CellValue::deserialize)
                        .transpose()
                        .map_err(serde::de::Error::custom)?
                        .map(Ok)
                        .unwrap_or(Ok(CellValue::Null));
                }

                if object.get("type").and_then(Value::as_str) == Some("formula") {
                    let formula = object
                        .remove("formula")
                        .and_then(|value| value.as_str().map(ToOwned::to_owned))
                        .unwrap_or_default();
                    let cached_value = object
                        .remove("cachedValue")
                        .or_else(|| object.remove("cached_value"))
                        .map(CellValue::deserialize)
                        .transpose()
                        .map_err(serde::de::Error::custom)?
                        .unwrap_or(CellValue::Null);
                    let error = object
                        .remove("error")
                        .and_then(|value| value.as_str().map(ToOwned::to_owned));

                    Ok(CellValue::Formula {
                        formula: normalize_formula_text(formula),
                        cached_value: Box::new(cached_value),
                        error,
                    })
                } else {
                    // 不支持的对象类型，转为字符串
                    Ok(CellValue::String(Value::Object(object).to_string()))
                }
            }
            Value::Array(_) => {
                // 不支持的类型，转为字符串
                Ok(CellValue::String(value.to_string()))
            }
        }
    }
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

#[derive(Deserialize, TS, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SheetData {
    pub name: String,
    pub rows: Vec<Vec<CellValue>>,
    /// 合并范围
    pub merges: Vec<MergeRange>,
    /// 列宽配置（用于持久化）
    #[serde(default)]
    #[ts(optional)]
    pub column_widths: Option<HashMap<usize, u32>>,
    /// 行高配置（持久化到 Excel，属于文档状态）
    #[serde(default)]
    #[ts(optional)]
    pub row_heights: Option<HashMap<usize, u32>>,
    /// Read-only rich Excel projection. This is display metadata only; the
    /// original workbook remains the persistence source for styles and drawings.
    #[serde(default)]
    pub rich: ReadOnlyRichProjection,
}

impl Serialize for SheetData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("SheetData", 6)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("rows", &SheetRowsProjection { sheet: self })?;
        state.serialize_field("merges", &self.merges)?;
        if let Some(column_widths) = &self.column_widths {
            state.serialize_field("columnWidths", column_widths)?;
        }
        if let Some(row_heights) = &self.row_heights {
            state.serialize_field("rowHeights", row_heights)?;
        }
        state.serialize_field("rich", &self.rich)?;
        state.end()
    }
}

impl SheetData {
    pub fn cell_format_at(&self, row: usize, col: usize) -> Option<CellFormatProjection> {
        let key = excel_cell_key(row, col);
        let explicit = self.rich.cell_formats.get(&key);
        let style_number_format = self
            .rich
            .cell_styles
            .get(&key)
            .and_then(|style| style.number_format.clone());

        if explicit.is_none() && style_number_format.is_none() {
            return None;
        }

        Some(CellFormatProjection {
            number_format: explicit
                .and_then(|format| format.number_format.clone())
                .or(style_number_format),
            style_id: explicit.and_then(|format| format.style_id.clone()),
        })
    }

    pub fn cell_style_at(&self, row: usize, col: usize) -> Option<CellStyleProjection> {
        self.rich
            .cell_styles
            .get(&excel_cell_key(row, col))
            .cloned()
    }

    pub fn cell_display_text(&self, row: usize, col: usize) -> String {
        self.rows
            .get(row)
            .and_then(|row_data| row_data.get(col))
            .map(|cell| {
                DisplayProjection::display_text(cell, self.cell_format_at(row, col).as_ref())
            })
            .unwrap_or_default()
    }

    pub fn cell_search_text(&self, row: usize, col: usize) -> String {
        self.rows
            .get(row)
            .and_then(|row_data| row_data.get(col))
            .map(|cell| {
                DisplayProjection::search_text(cell, self.cell_format_at(row, col).as_ref())
            })
            .unwrap_or_default()
    }
}

fn excel_cell_key(row_index: usize, col_index: usize) -> String {
    let mut col = col_index + 1;
    let mut letters = String::new();
    while col > 0 {
        let rem = (col - 1) % 26;
        letters.insert(0, (b'A' + rem as u8) as char);
        col = (col - 1) / 26;
    }
    format!("{letters}{}", row_index + 1)
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FileData {
    pub path: String,
    pub file_name: String,
    pub sheets: Vec<SheetData>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct OpenDocumentResponse {
    pub file_data: FileData,
    pub editor_session: EditorSessionInfo,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SavedDocumentResponse {
    pub file_data: FileData,
    pub editor_session: EditorSessionInfo,
}

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

/// 单元格变化
#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CellChange {
    pub row: usize,
    pub col: usize,
    pub value: CellValue,
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
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub display: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub format: Option<CellFormatProjection>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub style: Option<CellStyleProjection>,
    #[serde(default)]
    #[ts(skip)]
    pub display_format: Option<CellFormatProjection>,
}

impl SheetCellChange {
    pub fn new(sheet_index: usize, row: usize, col: usize, value: CellValue) -> Self {
        Self {
            sheet_index,
            row,
            col,
            value,
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
        let mut len = 4;
        if self.display.is_some() {
            len += 1;
        }
        if self.format.is_some() {
            len += 1;
        }
        if self.style.is_some() {
            len += 1;
        }

        let mut state = serializer.serialize_struct("SheetCellChange", len)?;
        state.serialize_field("sheetIndex", &self.sheet_index)?;
        state.serialize_field("row", &self.row)?;
        state.serialize_field("col", &self.col)?;
        state.serialize_field(
            "value",
            &CellValueProjection::new(&self.value, self.display_format.clone()),
        )?;
        if let Some(display) = &self.display {
            state.serialize_field("display", display)?;
        }
        if let Some(format) = &self.format {
            state.serialize_field("format", format)?;
        }
        if let Some(style) = &self.style {
            state.serialize_field("style", style)?;
        }
        state.end()
    }
}

/// 前端批量提交的单元格编辑请求。
#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SetCellRequest {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub text: String,
}

/// 行变化
#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RowChange {
    pub index: usize,
    pub values: Vec<CellValue>,
}

/// 列变化
#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[ts(rename_all = "camelCase")]
pub struct ColumnChange {
    pub index: usize,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ColumnWidthChange {
    #[serde(rename = "colIndex")]
    pub col_index: usize,
    pub width: Option<u32>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RowHeightChange {
    #[serde(rename = "rowIndex")]
    pub row_index: usize,
    pub height: Option<u32>,
}

// ==================== Applied Operation Result ====================

/// Internal result produced after an editor operation has been applied.
#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(tag = "type", content = "data")]
#[ts(tag = "type", content = "data")]
pub enum AppliedOperationResult {
    /// 单元格修改
    #[serde(rename = "SetCell")]
    SetCell {
        #[serde(rename = "sheetIndex")]
        sheet_index: usize,
        cell: CellChange,
    },
    #[serde(rename = "SetCells")]
    SetCells { changes: Vec<SheetCellChange> },
    /// 添加行
    #[serde(rename = "AddRow")]
    AddRow {
        #[serde(rename = "sheetIndex")]
        sheet_index: usize,
        row: RowChange,
    },
    /// 删除行
    #[serde(rename = "DeleteRow")]
    DeleteRow {
        #[serde(rename = "sheetIndex")]
        sheet_index: usize,
        #[serde(rename = "rowIndex")]
        row_index: usize,
    },
    /// 添加列
    #[serde(rename = "AddColumn")]
    AddColumn {
        #[serde(rename = "sheetIndex")]
        sheet_index: usize,
        column: ColumnChange,
        /// 添加的列数据（用于撤销时恢复）
        #[serde(rename = "colData")]
        col_data: Vec<CellValue>,
    },
    /// 删除列
    #[serde(rename = "DeleteColumn")]
    DeleteColumn {
        #[serde(rename = "sheetIndex")]
        sheet_index: usize,
        #[serde(rename = "columnIndex")]
        column_index: usize,
    },
    #[serde(rename = "SetColumnWidth")]
    SetColumnWidth {
        #[serde(rename = "sheetIndex")]
        sheet_index: usize,
        column: ColumnWidthChange,
    },
    #[serde(rename = "SetRowHeight")]
    SetRowHeight {
        #[serde(rename = "sheetIndex")]
        sheet_index: usize,
        row: RowHeightChange,
    },
    /// 添加 Sheet
    #[serde(rename = "AddSheet")]
    AddSheet {
        #[serde(rename = "sheetIndex")]
        sheet_index: usize,
        name: String,
        /// 完整的 sheet 数据（用于撤销时恢复）
        #[serde(rename = "sheetData")]
        sheet_data: SheetData,
    },
    /// 删除 Sheet
    #[serde(rename = "DeleteSheet")]
    DeleteSheet {
        #[serde(rename = "sheetIndex")]
        sheet_index: usize,
        /// 被删除的 sheet 数据（用于撤销时恢复）
        #[serde(rename = "sheetData")]
        sheet_data: SheetData,
    },
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
    pub sheet: SheetData,
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
pub struct SheetUpdatedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    pub sheet: SheetData,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SheetsReplacedPatch {
    #[serde(rename = "startIndex")]
    pub start_index: usize,
    pub sheets: Vec<SheetData>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(tag = "type", rename_all = "camelCase")]
pub enum RichProjectionPatchScope {
    All,
    Rows { start: usize },
    Columns { start: usize },
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RichProjectionPatch {
    pub scope: RichProjectionPatchScope,
    pub projection: ReadOnlyRichProjection,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SheetStructureMetadataPatch {
    pub merges: Vec<MergeRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub column_widths: Option<HashMap<usize, u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub row_heights: Option<HashMap<usize, u32>>,
    pub rich: RichProjectionPatch,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RowInsertedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    #[serde(rename = "rowIndex")]
    pub row_index: usize,
    pub rows: Vec<Vec<CellValue>>,
    pub metadata: SheetStructureMetadataPatch,
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
    pub metadata: SheetStructureMetadataPatch,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ColumnInsertedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    #[serde(rename = "colIndex")]
    pub col_index: usize,
    pub values: Vec<CellValue>,
    pub metadata: SheetStructureMetadataPatch,
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
    pub metadata: SheetStructureMetadataPatch,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SheetShapePatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    pub row_lengths: Vec<usize>,
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
    #[serde(rename = "SheetUpdated")]
    SheetUpdated { patch: SheetUpdatedPatch },
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
    #[serde(rename = "SheetShape")]
    SheetShape { patch: SheetShapePatch },
    #[serde(rename = "ResyncRequired")]
    ResyncRequired { patch: ResyncRequiredPatch },
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct EditorMutationResponse {
    #[ts(type = "1")]
    pub protocol_version: u16,
    #[ts(type = "number")]
    pub document_id: u64,
    #[ts(type = "number")]
    pub revision: u64,
    pub formula_status: FormulaStatus,
    #[serde(default)]
    pub capabilities: WorkbookCapabilities,
    pub editor_state: EditorStateInfo,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patches: Vec<EditorPatch>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn parses_user_cell_text_on_backend() {
        assert_eq!(parse_cell_text(""), CellValue::Null);
        assert_eq!(parse_cell_text("007"), CellValue::String("007".to_string()));
        assert_eq!(parse_cell_text("42"), CellValue::Number(Value::from(42)));
        assert_eq!(parse_cell_text("3.5"), CellValue::Number(Value::from(3.5)));
        assert_eq!(parse_cell_text("true"), CellValue::Boolean(true));
        assert_eq!(
            parse_cell_text("=A1+1"),
            CellValue::formula("=A1+1", CellValue::Null)
        );
    }

    #[test]
    fn sheet_serialization_projects_formatted_cell_display() {
        let sheet = SheetData {
            name: "Sheet1".to_string(),
            rows: vec![vec![CellValue::Number(Value::from(0.4))]],
            rich: ReadOnlyRichProjection {
                cell_formats: HashMap::from([(
                    "A1".to_string(),
                    CellFormatProjection {
                        number_format: Some("0%".to_string()),
                        style_id: None,
                    },
                )]),
                ..Default::default()
            },
            ..Default::default()
        };

        let json = serde_json::to_value(&sheet).expect("serialize sheet");
        assert_eq!(json["rows"][0][0]["display"], "40%");
        assert_eq!(json["rows"][0][0]["raw"], 0.4);
        assert_eq!(json["rows"][0][0]["format"]["numberFormat"], "0%");
    }
}
