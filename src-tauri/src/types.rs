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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SheetData {
    pub name: String,
    pub rows: Vec<Vec<CellValue>>,
    #[serde(skip)]
    pub index: SheetIndex,
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
    },
    /// 删除列
    DeleteColumn {
        sheet_index: usize,
        column_index: usize,
    },
    /// 批量变化（用于 undo/redo）
    Batch {
        sheet_index: usize,
        changes: Vec<CellChange>,
    },
}
