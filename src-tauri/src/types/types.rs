use crate::state::state::EditorStateInfo;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

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

impl Serialize for CellValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let mut len = 4;
        if matches!(self, CellValue::Formula { .. }) {
            len += 1;
        }

        let mut map = serializer.serialize_map(Some(len))?;
        map.serialize_entry("type", "cell")?;
        map.serialize_entry("kind", self.kind())?;
        map.serialize_entry("raw", &self.raw_json_value())?;
        map.serialize_entry("display", &self.to_display_string())?;
        if let CellValue::Formula {
            formula,
            cached_value,
            error,
        } = self
        {
            let formula_projection = FormulaCellProjection {
                formula,
                cached_value,
                error: error.as_deref(),
            };
            map.serialize_entry("formula", &formula_projection)?;
        }
        map.end()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FormulaCellProjection<'a> {
    formula: &'a str,
    cached_value: &'a CellValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a str>,
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

    fn raw_json_value(&self) -> Value {
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

/// 单元格位置
#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq)]
pub struct CellPosition {
    pub row: usize,
    pub col: usize,
}

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

/// 搜索范围
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub enum SearchScope {
    CurrentSheet,
    AllSheets,
}

/// 合并范围
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MergeRange {
    pub start_row: u32,
    pub start_col: u16,
    pub end_row: u32,
    pub end_col: u16,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SheetData {
    pub name: String,
    pub rows: Vec<Vec<CellValue>>,
    /// 合并范围
    pub merges: Vec<MergeRange>,
    /// 列宽配置（用于持久化）
    #[serde(default)]
    pub column_widths: Option<HashMap<usize, u32>>,
    /// 行高配置（持久化到 Excel，属于文档状态）
    #[serde(default)]
    pub row_heights: Option<HashMap<usize, u32>>,
    /// Optional rich Excel projection. This is display metadata only; the
    /// original workbook remains the persistence source for styles and drawings.
    #[serde(default)]
    pub rich: SheetRichProjection,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileData {
    pub path: String,
    pub file_name: String,
    pub sheets: Vec<SheetData>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SheetRichProjection {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub cell_styles: HashMap<String, CellStyleProjection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drawings: Vec<DrawingProjection>,
    #[serde(default)]
    pub has_more_drawings: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
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

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DrawingProjection {
    pub kind: DrawingKind,
    pub from_row: u32,
    pub from_col: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_row: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_col: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub enum DrawingKind {
    Image,
    Chart,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DocumentCapabilities {
    pub native_save_extension: Option<String>,
    pub export_extension: String,
    pub requires_save_as_for_native_save: bool,
    #[serde(default)]
    pub workbook: WorkbookCapabilities,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkbookCapabilities {
    pub can_edit_cells: bool,
    pub can_resize_rows_columns: bool,
    pub can_edit_structure: bool,
    pub can_native_save: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_structure_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detected_features: Vec<String>,
}

impl Default for WorkbookCapabilities {
    fn default() -> Self {
        Self {
            can_edit_cells: true,
            can_resize_rows_columns: true,
            can_edit_structure: true,
            can_native_save: true,
            blocked_structure_reasons: Vec::new(),
            detected_features: Vec::new(),
        }
    }
}

/// 单元格变化
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CellChange {
    pub row: usize,
    pub col: usize,
    pub value: CellValue,
}

/// 带 sheet 的单元格变化，用于高频编辑的增量响应。
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SheetCellChange {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub value: CellValue,
}

/// 前端批量提交的单元格编辑请求。
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SetCellRequest {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub text: String,
}

/// 行变化
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RowChange {
    pub index: usize,
    pub values: Vec<CellValue>,
}

/// 列变化
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ColumnChange {
    pub index: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ColumnWidthChange {
    #[serde(rename = "colIndex")]
    pub col_index: usize,
    pub width: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RowHeightChange {
    #[serde(rename = "rowIndex")]
    pub row_index: usize,
    pub height: Option<u32>,
}

// ==================== GitHub Update Types ====================

/// 更新信息，返回给前端
#[cfg(any(target_os = "android", target_os = "ios"))]
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub version: String,
    pub tag_name: String,
    pub release_url: String,
    pub apk_url: Option<String>,
}

/// GitHub Release API 响应结构
#[cfg(any(target_os = "android", target_os = "ios"))]
#[derive(Debug, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub assets: Vec<GitHubAsset>,
}

/// GitHub Release Asset 结构
#[cfg(any(target_os = "android", target_os = "ios"))]
#[derive(Debug, Deserialize)]
pub struct GitHubAsset {
    pub name: String,
    pub browser_download_url: String,
}

// ==================== Applied Operation Result ====================

/// Internal result produced after an editor operation has been applied.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", content = "data")]
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
pub struct RowInsertedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    #[serde(rename = "rowIndex")]
    pub row_index: usize,
    pub row: Vec<CellValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_height: Option<u32>,
    pub merges: Vec<MergeRange>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub row_heights: HashMap<usize, u32>,
    #[serde(default)]
    pub rich: SheetRichProjection,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RowDeletedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    #[serde(rename = "rowIndex")]
    pub row_index: usize,
    pub merges: Vec<MergeRange>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub row_heights: HashMap<usize, u32>,
    #[serde(default)]
    pub rich: SheetRichProjection,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ColumnInsertedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    #[serde(rename = "colIndex")]
    pub col_index: usize,
    pub column: Vec<CellValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_width: Option<u32>,
    pub merges: Vec<MergeRange>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub column_widths: HashMap<usize, u32>,
    #[serde(default)]
    pub rich: SheetRichProjection,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ColumnDeletedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    #[serde(rename = "colIndex")]
    pub col_index: usize,
    pub merges: Vec<MergeRange>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub column_widths: HashMap<usize, u32>,
    #[serde(default)]
    pub rich: SheetRichProjection,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SheetInsertedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    pub sheet: SheetData,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SheetDeletedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", content = "data")]
pub enum EditorPatch {
    #[serde(rename = "Cells")]
    Cells { changes: Vec<SheetCellChange> },
    #[serde(rename = "Layout")]
    Layout { patch: LayoutPatch },
    #[serde(rename = "RowInserted")]
    RowInserted { patch: RowInsertedPatch },
    #[serde(rename = "RowDeleted")]
    RowDeleted { patch: RowDeletedPatch },
    #[serde(rename = "ColumnInserted")]
    ColumnInserted { patch: ColumnInsertedPatch },
    #[serde(rename = "ColumnDeleted")]
    ColumnDeleted { patch: ColumnDeletedPatch },
    #[serde(rename = "SheetInserted")]
    SheetInserted { patch: SheetInsertedPatch },
    #[serde(rename = "SheetDeleted")]
    SheetDeleted { patch: SheetDeletedPatch },
    #[serde(rename = "SheetSnapshot")]
    SheetSnapshot {
        #[serde(rename = "sheetIndex")]
        sheet_index: usize,
        sheet: SheetData,
    },
    #[serde(rename = "FullSnapshot")]
    FullSnapshot {
        #[serde(rename = "fileData")]
        file_data: FileData,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EditorMutationResponse {
    pub protocol_version: u16,
    pub document_id: u64,
    pub revision: u64,
    pub formula_status: FormulaStatus,
    #[serde(default)]
    pub capabilities: WorkbookCapabilities,
    pub editor_state: EditorStateInfo,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patches: Vec<EditorPatch>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FormulaDiagnostics {
    pub invalid_formula_count: usize,
    pub volatile_formula_count: usize,
    pub unsupported_dependency_count: usize,
    pub large_range_dependency_count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
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

impl FormulaStatus {
    pub fn ready(diagnostics: FormulaDiagnostics) -> Self {
        Self::Ready { diagnostics }
    }

    pub fn degraded(message: String, diagnostics: FormulaDiagnostics) -> Self {
        Self::Degraded {
            message,
            diagnostics,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
