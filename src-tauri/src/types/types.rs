use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum CellValue {
    Null,
    String(String),
    Number(f64),
    Boolean(bool),
}

/// 单元格位置
#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq)]
pub struct CellPosition {
    pub row: usize,
    pub col: usize,
}

/// 搜索结果
#[derive(Serialize, Deserialize, Clone, Debug)]
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
    pub inverted_index: HashMap<String, Vec<CellPosition>>,
}

/// 合并范围
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MergeRange {
    pub start_row: u32,
    pub start_col: u16,
    pub end_row: u32,
    pub end_col: u16,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SheetData {
    pub name: String,
    pub rows: Vec<Vec<CellValue>>,
    /// 合并范围
    pub merges: Vec<MergeRange>,
    #[serde(skip)]
    pub index: SheetIndex,
}

impl SheetData {
    /// 判断是否为空的 sheet（用于判断是否需要保存数据）
    /// 只有当 name 为空且 rows 也为空时，才认为是空的 sheet
    pub fn is_empty(&self) -> bool {
        self.name.is_empty() && self.rows.is_empty()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FileData {
    pub file_name: String,
    pub sheets: Vec<SheetData>,
}

/// 单元格变化
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CellChange {
    pub row: usize,
    pub col: usize,
    pub value: CellValue,
}

/// 行变化
#[derive(Serialize, Deserialize, Clone, Debug)]
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
pub struct SortState {
    pub col_index: usize,
    pub ascending: bool,
}

/// 操作结果（增量数据）
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", content = "data")]
pub enum OperationResult {
    /// 单元格修改
    SetCell {
        sheet_index: usize,
        cell: CellChange,
    },
    /// 添加行
    AddRow {
        sheet_index: usize,
        row: RowChange,
    },
    /// 删除行
    DeleteRow {
        sheet_index: usize,
        row_index: usize,
    },
    /// 添加列
    AddColumn {
        sheet_index: usize,
        column: ColumnChange,
        /// 添加的列数据（用于撤销时恢复）
        col_data: Vec<CellValue>,
    },
    /// 删除列
    DeleteColumn {
        sheet_index: usize,
        column_index: usize,
    },
    /// 添加 Sheet
    AddSheet {
        sheet_index: usize,
        name: String,
        /// 完整的 sheet 数据（用于撤销时恢复）
        sheet_data: SheetData,
    },
    /// 删除 Sheet
    DeleteSheet {
        sheet_index: usize,
        /// 被删除的 sheet 数据（用于撤销时恢复）
        sheet_data: SheetData,
    },
    /// 列排序
    SortColumn {
        sheet_index: usize,
        sheet_data: SheetData,
        sort_state: Option<SortState>,
    },
}
