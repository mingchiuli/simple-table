use crate::types::{CellValue, SetCellRequest, SheetData};
use serde::{Deserialize, Serialize};

/// User-facing editor command.
///
/// This is intentionally not a history record. Undo/redo is handled by
/// document mementos, so command variants only describe the requested mutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EditorCommand {
    SetCell {
        sheet_index: usize,
        row: usize,
        col: usize,
        text: String,
    },
    SetCells {
        changes: Vec<SetCellRequest>,
    },
    AddRow {
        sheet_index: usize,
        row_index: usize,
    },
    DeleteRow {
        sheet_index: usize,
        row_index: usize,
    },
    AddColumn {
        sheet_index: usize,
    },
    DeleteColumn {
        sheet_index: usize,
        col_index: usize,
    },
    SetColumnWidth {
        sheet_index: usize,
        col_index: usize,
        width: Option<u32>,
    },
    SetRowHeight {
        sheet_index: usize,
        row_index: usize,
        height: Option<u32>,
    },
    AddSheet {
        name: Option<String>,
    },
    DeleteSheet {
        sheet_index: usize,
    },
}

/// Canonical mutation after resolving indices and current document state.
#[derive(Debug, Clone)]
pub enum AppliedOperation {
    SetCell {
        sheet_index: usize,
        row: usize,
        col: usize,
        old_value: CellValue,
        new_value: CellValue,
    },
    SetCells {
        changes: Vec<ResolvedCellEdit>,
    },
    AddRow {
        sheet_index: usize,
        row_index: usize,
        row_data: Vec<CellValue>,
        row_height: Option<u32>,
    },
    DeleteRow {
        sheet_index: usize,
        row_index: usize,
    },
    AddColumn {
        sheet_index: usize,
        col_index: usize,
        col_data: Vec<CellValue>,
        column_width: Option<u32>,
    },
    DeleteColumn {
        sheet_index: usize,
        col_index: usize,
    },
    SetColumnWidth {
        sheet_index: usize,
        col_index: usize,
        old_width: Option<u32>,
        new_width: Option<u32>,
    },
    SetRowHeight {
        sheet_index: usize,
        row_index: usize,
        old_height: Option<u32>,
        new_height: Option<u32>,
    },
    AddSheet {
        sheet_index: usize,
        sheet_data: SheetData,
    },
    DeleteSheet {
        sheet_index: usize,
    },
}

#[derive(Debug, Clone)]
pub struct ResolvedCellEdit {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub old_value: CellValue,
    pub new_value: CellValue,
}

pub struct ProjectionMutation<'a> {
    pub(crate) operation: &'a AppliedOperation,
}

pub struct OperationPatchProjector<'a> {
    pub(crate) operation: &'a AppliedOperation,
}

pub struct MutationImpact<'a> {
    pub(crate) operation: &'a AppliedOperation,
}

impl AppliedOperation {
    pub fn projection_mutation(&self) -> ProjectionMutation<'_> {
        ProjectionMutation { operation: self }
    }

    pub fn patch_projector(&self) -> OperationPatchProjector<'_> {
        OperationPatchProjector { operation: self }
    }

    pub fn impact(&self) -> MutationImpact<'_> {
        MutationImpact { operation: self }
    }
}
