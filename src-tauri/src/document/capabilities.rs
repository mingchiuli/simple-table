#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SheetCapabilities {
    pub can_edit_cells: bool,
    pub can_resize_rows_columns: bool,
    pub can_insert_delete_rows: bool,
    pub can_insert_delete_columns: bool,
    pub blocked_edit_reasons: Vec<String>,
    pub blocked_resize_reasons: Vec<String>,
    pub blocked_row_structure_reasons: Vec<String>,
    pub blocked_column_structure_reasons: Vec<String>,
}

impl Default for SheetCapabilities {
    fn default() -> Self {
        Self {
            can_edit_cells: true,
            can_resize_rows_columns: true,
            can_insert_delete_rows: true,
            can_insert_delete_columns: true,
            blocked_edit_reasons: Vec::new(),
            blocked_resize_reasons: Vec::new(),
            blocked_row_structure_reasons: Vec::new(),
            blocked_column_structure_reasons: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkbookSaveCapabilities {
    pub can_native_save: bool,
    pub blocked_save_reasons: Vec<String>,
    pub detected_features: Vec<String>,
}

impl Default for WorkbookSaveCapabilities {
    fn default() -> Self {
        Self {
            can_native_save: true,
            blocked_save_reasons: Vec::new(),
            detected_features: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkbookStructureCapabilities {
    pub can_insert_delete_sheets: bool,
    pub blocked_structure_reasons: Vec<String>,
    pub blocked_sheet_structure_reasons: Vec<String>,
}

impl Default for WorkbookStructureCapabilities {
    fn default() -> Self {
        Self {
            can_insert_delete_sheets: true,
            blocked_structure_reasons: Vec::new(),
            blocked_sheet_structure_reasons: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct WorkbookRichCapabilities {
    pub can_edit_styles: bool,
    pub can_edit_drawings: bool,
    pub can_edit_hyperlinks: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct WorkbookCapabilities {
    pub save: WorkbookSaveCapabilities,
    pub structure: WorkbookStructureCapabilities,
    pub rich: WorkbookRichCapabilities,
    pub sheets: Vec<SheetCapabilities>,
}
