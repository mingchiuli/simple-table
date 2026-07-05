#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FormulaCellRef {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
}
