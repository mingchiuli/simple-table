use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum CellValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Empty,
}

#[derive(Serialize, Deserialize)]
pub struct SheetData {
    pub name: String,
    pub rows: Vec<Vec<CellValue>>,
}

#[derive(Serialize, Deserialize)]
pub struct FileData {
    pub file_name: String,
    pub sheets: Vec<SheetData>,
}
