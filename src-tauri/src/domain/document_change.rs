use crate::domain::CellValue;

#[derive(Clone, Debug, PartialEq)]
pub struct DocumentCellChange {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub value: CellValue,
}

impl DocumentCellChange {
    pub fn new(sheet_index: usize, row: usize, col: usize, value: CellValue) -> Self {
        Self {
            sheet_index,
            row,
            col,
            value,
        }
    }
}
