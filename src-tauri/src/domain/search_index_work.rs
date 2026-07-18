#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SearchIndexWork {
    #[default]
    None,
    UpdateCells(Vec<SearchCellIndexUpdate>),
    RebuildAll,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchCellIndexUpdate {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub search_text: String,
    pub display_text: String,
}
