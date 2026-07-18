use std::collections::HashMap;

use crate::domain::DocumentCellChange;

#[derive(Debug, Clone)]
pub struct RestoredSheet {
    pub name: String,
    pub row_count: usize,
    pub column_count: usize,
    pub column_widths: HashMap<usize, u32>,
    pub row_heights: HashMap<usize, u32>,
}

#[derive(Debug, Clone)]
pub enum DocumentRestoreChange {
    Cells(Vec<DocumentCellChange>),
    Layout {
        sheet_index: usize,
        column_widths: HashMap<usize, Option<u32>>,
        row_heights: HashMap<usize, Option<u32>>,
    },
    RowInserted {
        sheet_index: usize,
        row_index: usize,
        count: usize,
    },
    RowDeleted {
        sheet_index: usize,
        row_index: usize,
        count: usize,
    },
    ColumnInserted {
        sheet_index: usize,
        col_index: usize,
        count: usize,
    },
    ColumnDeleted {
        sheet_index: usize,
        col_index: usize,
        count: usize,
    },
    SheetsReplaced {
        start_index: usize,
        sheets: Vec<RestoredSheet>,
    },
    SheetInvalidated {
        sheet_index: usize,
    },
    ResyncRequired {
        reason: String,
    },
}

#[derive(Debug, Clone, Default)]
pub struct DocumentRestoreResult {
    pub changes: Vec<DocumentRestoreChange>,
}
