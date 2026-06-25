use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use crate::state::state::EditorStateInfo;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
        match self {
            CellValue::Null => serializer.serialize_none(),
            CellValue::String(s) => serializer.serialize_str(s),
            CellValue::Number(v) => {
                // 如果是整数且超出 JavaScript 安全范围，序列化为字符串
                if let Some(i) = v.as_i64()
                    && !(JS_MIN_SAFE_INTEGER..=JS_MAX_SAFE_INTEGER).contains(&i)
                {
                    return serializer.serialize_str(&i.to_string());
                }
                // 否则正常序列化
                v.serialize(serializer)
            }
            CellValue::Boolean(b) => serializer.serialize_bool(*b),
            CellValue::Formula {
                formula,
                cached_value,
                error,
            } => {
                use serde::ser::SerializeMap;

                let mut len = 3;
                if error.is_some() {
                    len += 1;
                }
                let mut map = serializer.serialize_map(Some(len))?;
                map.serialize_entry("type", "formula")?;
                map.serialize_entry("formula", formula)?;
                map.serialize_entry("cachedValue", cached_value)?;
                if let Some(error) = error {
                    map.serialize_entry("error", error)?;
                }
                map.end()
            }
        }
    }
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

/// Sheet 索引（不序列化）
#[derive(Default)]
pub struct SheetIndex {
    /// 全文搜索索引
    pub search_index: Option<tantivy::Index>,
    /// Schema
    pub search_schema: Option<tantivy::schema::Schema>,
    /// 文本字段
    pub text_field: Option<tantivy::schema::Field>,
    /// 单元格主键字段（"row:col"，用于增量 delete_term）
    pub cell_id_field: Option<tantivy::schema::Field>,
    /// 持久 IndexWriter，跨编辑复用以支持增量更新
    pub writer: Option<Arc<Mutex<tantivy::IndexWriter>>>,
}

impl fmt::Debug for SheetIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SheetIndex")
            .field("search_index", &self.search_index.is_some())
            .field("search_schema", &self.search_schema.is_some())
            .field("text_field", &self.text_field)
            .field("cell_id_field", &self.cell_id_field)
            .field("writer", &self.writer.is_some())
            .finish()
    }
}

impl Clone for SheetIndex {
    /// 克隆时跳过 writer（IndexWriter 不可 Clone；undo 历史中的 SheetData 不需要写）
    /// 拿到克隆体后若需要写入索引，调用方应触发一次全量重建以生成新 writer
    fn clone(&self) -> Self {
        Self {
            search_index: self.search_index.clone(),
            search_schema: self.search_schema.clone(),
            text_field: self.text_field,
            cell_id_field: self.cell_id_field,
            writer: None,
        }
    }
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
    /// 运行时索引（不序列化）
    #[serde(skip)]
    pub index: SheetIndex,
    /// 列宽配置（用于持久化）
    #[serde(default)]
    pub column_widths: Option<HashMap<usize, u32>>,
    /// 行高配置（用于前端布局，不计入内容 dirty）
    #[serde(default)]
    pub row_heights: Option<HashMap<usize, u32>>,
}

impl SheetData {
    /// 判断是否为空的 sheet（用于判断是否需要保存数据）
    /// 只有当 name 为空且 rows 也为空时，才认为是空的 sheet
    pub fn is_empty(&self) -> bool {
        self.name.is_empty() && self.rows.is_empty()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileData {
    pub path: String,
    pub file_name: String,
    pub sheets: Vec<SheetData>,
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

/// 排序状态
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SortState {
    pub col_index: usize,
    pub ascending: bool,
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

// ==================== Operation Result ====================

/// 操作结果（增量数据）
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", content = "data")]
pub enum OperationResult {
    /// 单元格修改
    #[serde(rename = "SetCell")]
    SetCell {
        #[serde(rename = "sheetIndex")]
        sheet_index: usize,
        cell: CellChange,
    },
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
    /// 列排序
    #[serde(rename = "SortColumn")]
    SortColumn {
        #[serde(rename = "sheetIndex")]
        sheet_index: usize,
        #[serde(rename = "sheetData")]
        sheet_data: SheetData,
        #[serde(rename = "sortState")]
        sort_state: Option<SortState>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub enum EditorMutationResponseKind {
    Snapshot,
    CellDelta,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EditorMutationResponse {
    pub kind: EditorMutationResponseKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_data: Option<FileData>,
    pub editor_state: EditorStateInfo,
    pub operation: Option<OperationResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cell_changes: Vec<SheetCellChange>,
}
