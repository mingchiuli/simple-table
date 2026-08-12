#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SearchScope {
    CurrentSheet,
    AllSheets,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SearchHit {
    pub sheet_index: usize,
    pub sheet_name: String,
    pub row: usize,
    pub col: usize,
    pub value: String,
    pub cell_position: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SearchOutcome {
    pub results: Vec<SearchHit>,
    pub truncated: bool,
}
