#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct StructurePatchDiagnostics {
    pub(crate) skipped_formula_reference_rewrites: usize,
}

pub(crate) struct WorkbookSheetShape {
    pub(crate) sheet_index: usize,
    pub(crate) row_lengths: Vec<usize>,
    pub(crate) protected_cells: Vec<(usize, usize)>,
}
