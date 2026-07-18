#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchCellText {
    pub row: usize,
    pub col: usize,
    pub search_text: String,
    pub display_text: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchScanCursor {
    pub row: usize,
    pub col: usize,
}

pub struct SearchTextChunk {
    pub cells: Vec<SearchCellText>,
    pub next: Option<SearchScanCursor>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchSheetSnapshot {
    pub name: String,
    pub estimated_source_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchDocumentSnapshot {
    pub revision: u64,
    pub sheets: Vec<SearchSheetSnapshot>,
}
