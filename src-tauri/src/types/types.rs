use crate::state::state::{EditorSessionInfo, EditorStateInfo};
use serde::ser::{SerializeMap, SerializeStruct};
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

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CellFormatProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_id: Option<String>,
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

    pub fn to_display_string_with_format(&self, format: Option<&CellFormatProjection>) -> String {
        match self {
            CellValue::Formula {
                cached_value,
                error,
                ..
            } => error
                .clone()
                .unwrap_or_else(|| cached_value.to_display_string_with_format(format)),
            CellValue::Number(number) => format
                .and_then(|format| format.number_format.as_deref())
                .and_then(|pattern| format_number_with_excel_pattern(number, pattern))
                .unwrap_or_else(|| self.to_display_string()),
            _ => self.to_display_string(),
        }
    }

    pub fn to_search_string_with_format(&self, format: Option<&CellFormatProjection>) -> String {
        let display = self.to_display_string_with_format(format);
        let raw = self.to_display_string();
        if raw.is_empty() || raw == display {
            display
        } else if display.is_empty() {
            raw
        } else {
            format!("{display}\n{raw}")
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

fn format_number_with_excel_pattern(number: &Value, pattern: &str) -> Option<String> {
    let value = number.as_f64()?;
    if !value.is_finite() {
        return None;
    }

    let normalized = pattern.to_ascii_lowercase();
    if normalized.contains("yy") || normalized.contains("dd") || normalized.contains("m/") {
        return format_excel_date(value, pattern);
    }

    let percent = pattern.contains('%');
    let displayed = if percent { value * 100.0 } else { value };
    let decimals = decimal_places(pattern);
    let formatted = format_fixed_number(displayed, decimals, pattern.contains(','));
    let currency = pattern
        .chars()
        .find(|value| matches!(value, '$' | '¥' | '￥' | '€' | '£'))
        .map(|value| value.to_string())
        .unwrap_or_default();

    Some(format!(
        "{currency}{formatted}{}",
        if percent { "%" } else { "" }
    ))
}

fn decimal_places(pattern: &str) -> usize {
    let Some(decimal_start) = pattern.find('.') else {
        return 0;
    };
    pattern[decimal_start + 1..]
        .chars()
        .take_while(|value| matches!(value, '0' | '#'))
        .count()
}

fn format_fixed_number(value: f64, decimals: usize, use_grouping: bool) -> String {
    let formatted = format!("{value:.decimals$}");
    if !use_grouping {
        return formatted;
    }

    let (negative, unsigned) = formatted
        .strip_prefix('-')
        .map(|value| (true, value))
        .unwrap_or((false, formatted.as_str()));
    let (integer, fraction) = unsigned
        .split_once('.')
        .map(|(integer, fraction)| (integer, Some(fraction)))
        .unwrap_or((unsigned, None));

    let mut grouped = String::new();
    for (index, ch) in integer.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped.insert(0, ',');
        }
        grouped.insert(0, ch);
    }

    if negative {
        grouped.insert(0, '-');
    }
    if let Some(fraction) = fraction {
        grouped.push('.');
        grouped.push_str(fraction);
    }
    grouped
}

fn format_excel_date(value: f64, pattern: &str) -> Option<String> {
    let days = value.round() as i64;
    let (year, month, day) = excel_serial_date_to_ymd(days)?;
    if pattern.contains('/') {
        Some(format!("{year:04}/{month:02}/{day:02}"))
    } else {
        Some(format!("{year:04}-{month:02}-{day:02}"))
    }
}

fn excel_serial_date_to_ymd(days: i64) -> Option<(i32, u32, u32)> {
    // Excel's 1900 date system is represented here with the same 1899-12-30
    // epoch used by the frontend display formatter.
    civil_from_days(days - 25_569)
}

fn civil_from_days(days: i64) -> Option<(i32, u32, u32)> {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    Some((i32::try_from(year).ok()?, month as u32, day as u32))
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
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MergeRange {
    pub start_row: u32,
    pub start_col: u16,
    pub end_row: u32,
    pub end_col: u16,
}

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
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

    pub fn cell_display_text(&self, row: usize, col: usize) -> String {
        self.rows
            .get(row)
            .and_then(|row_data| row_data.get(col))
            .map(|cell| cell.to_display_string_with_format(self.cell_format_at(row, col).as_ref()))
            .unwrap_or_default()
    }

    pub fn cell_search_text(&self, row: usize, col: usize) -> String {
        self.rows
            .get(row)
            .and_then(|row_data| row_data.get(col))
            .map(|cell| cell.to_search_string_with_format(self.cell_format_at(row, col).as_ref()))
            .unwrap_or_default()
    }
}

struct SheetRowsProjection<'a> {
    sheet: &'a SheetData,
}

impl Serialize for SheetRowsProjection<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;

        let mut rows = serializer.serialize_seq(Some(self.sheet.rows.len()))?;
        for (row_index, row) in self.sheet.rows.iter().enumerate() {
            rows.serialize_element(&SheetRowProjection {
                sheet: self.sheet,
                row_index,
                row,
            })?;
        }
        rows.end()
    }
}

struct SheetRowProjection<'a> {
    sheet: &'a SheetData,
    row_index: usize,
    row: &'a [CellValue],
}

impl Serialize for SheetRowProjection<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;

        let mut row = serializer.serialize_seq(Some(self.row.len()))?;
        for (col_index, cell) in self.row.iter().enumerate() {
            row.serialize_element(&CellValueProjection {
                cell,
                format: self.sheet.cell_format_at(self.row_index, col_index),
            })?;
        }
        row.end()
    }
}

struct CellValueProjection<'a> {
    cell: &'a CellValue,
    format: Option<CellFormatProjection>,
}

impl Serialize for CellValueProjection<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut len = 4;
        if matches!(self.cell, CellValue::Formula { .. }) {
            len += 1;
        }
        if self.format.is_some() {
            len += 1;
        }

        let mut map = serializer.serialize_map(Some(len))?;
        map.serialize_entry("type", "cell")?;
        map.serialize_entry("kind", self.cell.kind())?;
        map.serialize_entry("raw", &self.cell.raw_json_value())?;
        map.serialize_entry(
            "display",
            &self
                .cell
                .to_display_string_with_format(self.format.as_ref()),
        )?;
        if let Some(format) = &self.format {
            map.serialize_entry("format", format)?;
        }
        if let CellValue::Formula {
            formula,
            cached_value,
            error,
        } = self.cell
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileData {
    pub path: String,
    pub file_name: String,
    pub sheets: Vec<SheetData>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OpenDocumentResponse {
    pub file_data: FileData,
    pub editor_session: EditorSessionInfo,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetRichProjection {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub cell_formats: HashMap<String, CellFormatProjection>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub cell_styles: HashMap<String, CellStyleProjection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drawings: Vec<DrawingProjection>,
    #[serde(default)]
    pub has_more_drawings: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
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
    #[serde(default)]
    pub can_insert_delete_rows: bool,
    #[serde(default)]
    pub can_insert_delete_columns: bool,
    #[serde(default)]
    pub can_insert_delete_sheets: bool,
    pub can_native_save: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_structure_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_edit_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_resize_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_row_structure_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_column_structure_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_sheet_structure_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detected_features: Vec<String>,
}

impl Default for WorkbookCapabilities {
    fn default() -> Self {
        Self {
            can_edit_cells: true,
            can_resize_rows_columns: true,
            can_insert_delete_rows: true,
            can_insert_delete_columns: true,
            can_insert_delete_sheets: true,
            can_native_save: true,
            blocked_structure_reasons: Vec::new(),
            blocked_edit_reasons: Vec::new(),
            blocked_resize_reasons: Vec::new(),
            blocked_row_structure_reasons: Vec::new(),
            blocked_column_structure_reasons: Vec::new(),
            blocked_sheet_structure_reasons: Vec::new(),
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
#[serde(rename_all = "camelCase")]
pub struct SheetUpdatedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    pub sheet: SheetData,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SheetMetadataPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    pub merges: Vec<MergeRange>,
    pub column_widths: HashMap<usize, u32>,
    pub row_heights: HashMap<usize, u32>,
    pub rich: SheetRichProjection,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RowsInsertedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    #[serde(rename = "rowIndex")]
    pub row_index: usize,
    pub rows: Vec<Vec<CellValue>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RowsDeletedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    #[serde(rename = "rowIndex")]
    pub row_index: usize,
    pub count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ColumnsInsertedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    #[serde(rename = "colIndex")]
    pub col_index: usize,
    pub values: Vec<CellValue>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ColumnsDeletedPatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    #[serde(rename = "colIndex")]
    pub col_index: usize,
    pub count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SheetShapePatch {
    #[serde(rename = "sheetIndex")]
    pub sheet_index: usize,
    pub row_lengths: Vec<usize>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ResyncRequiredPatch {
    pub reason: String,
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
    #[serde(rename = "SheetUpdated")]
    SheetUpdated { patch: SheetUpdatedPatch },
    #[serde(rename = "SheetMetadata")]
    SheetMetadata { patch: SheetMetadataPatch },
    #[serde(rename = "RowsInserted")]
    RowsInserted { patch: RowsInsertedPatch },
    #[serde(rename = "RowsDeleted")]
    RowsDeleted { patch: RowsDeletedPatch },
    #[serde(rename = "ColumnsInserted")]
    ColumnsInserted { patch: ColumnsInsertedPatch },
    #[serde(rename = "ColumnsDeleted")]
    ColumnsDeleted { patch: ColumnsDeletedPatch },
    #[serde(rename = "SheetShape")]
    SheetShape { patch: SheetShapePatch },
    #[serde(rename = "ResyncRequired")]
    ResyncRequired { patch: ResyncRequiredPatch },
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
    pub skipped_reference_rewrite_count: usize,
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
            rich: SheetRichProjection {
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
