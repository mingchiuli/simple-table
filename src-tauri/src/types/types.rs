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
    Number(Value),  // 使用 serde_json::Value 支持精确大整数
    Boolean(bool),
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
                if let Some(i) = v.as_i64() {
                    if i > JS_MAX_SAFE_INTEGER || i < JS_MIN_SAFE_INTEGER {
                        return serializer.serialize_str(&i.to_string());
                    }
                }
                // 否则正常序列化
                v.serialize(serializer)
            }
            CellValue::Boolean(b) => serializer.serialize_bool(*b),
        }
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
                // 优先尝试解析为 i64
                if let Ok(i) = s.parse::<i64>() {
                    // 如果超出 JS 安全范围，保持为字符串
                    if i > JS_MAX_SAFE_INTEGER || i < JS_MIN_SAFE_INTEGER {
                        return Ok(CellValue::String(s));
                    }
                    Ok(CellValue::Number(Value::from(i)))
                } else if let Ok(f) = s.parse::<f64>() {
                    Ok(CellValue::Number(Value::from(f)))
                } else {
                    Ok(CellValue::String(s))
                }
            }
            Value::Array(_) | Value::Object(_) => {
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
#[derive(Clone, Debug, Default)]
pub struct SheetIndex {
    /// 全文搜索索引
    pub search_index: Option<tantivy::Index>,
    /// Schema
    pub search_schema: Option<tantivy::schema::Schema>,
    /// 文本字段
    pub text_field: Option<tantivy::schema::Field>,
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
