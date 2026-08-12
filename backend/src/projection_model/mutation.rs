use std::collections::HashMap;

use crate::document_data::{SheetExtent, SheetImage};
use crate::projection_model::{EditorSessionSnapshot, ProjectedCellChange, SheetManifestSnapshot};

#[derive(Clone, Debug)]
pub(crate) enum MutationPatch {
    Cells {
        changes: Vec<ProjectedCellChange>,
    },
    Layout {
        sheet_index: usize,
        column_widths: HashMap<usize, Option<u32>>,
        row_heights: HashMap<usize, Option<u32>>,
    },
    SheetInserted {
        sheet_index: usize,
        sheet: SheetManifestSnapshot,
    },
    SheetDeleted {
        sheet_index: usize,
    },
    SheetInvalidated {
        sheet_index: usize,
    },
    SheetsReplaced {
        start_index: usize,
        sheets: Vec<SheetManifestSnapshot>,
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
    ImageUpserted {
        sheet_index: usize,
        image: SheetImage,
    },
    ImageDeleted {
        sheet_index: usize,
        image_id: String,
    },
    ResyncRequired {
        reason: String,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct MutationOutcome {
    pub document_id: u64,
    pub revision: u64,
    pub session: EditorSessionSnapshot,
    pub patches: Vec<MutationPatch>,
    pub sheet_extents: Option<Vec<SheetExtent>>,
}

impl MutationOutcome {
    pub(crate) fn require_resync(&mut self, reason: impl Into<String>) {
        self.patches = vec![MutationPatch::ResyncRequired {
            reason: reason.into(),
        }];
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(test)]
pub(crate) enum MutationLookupStatus {
    Pending,
    Completed,
    Failed,
    Missing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg(test)]
pub(crate) struct MutationFailure {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug)]
#[cfg(test)]
pub(crate) struct MutationLookup {
    pub status: MutationLookupStatus,
    pub response: Option<MutationOutcome>,
    pub error: Option<MutationFailure>,
}

#[cfg(test)]
impl MutationLookup {
    pub(crate) fn pending() -> Self {
        Self {
            status: MutationLookupStatus::Pending,
            response: None,
            error: None,
        }
    }

    pub(crate) fn completed(response: MutationOutcome) -> Self {
        Self {
            status: MutationLookupStatus::Completed,
            response: Some(response),
            error: None,
        }
    }

    pub(crate) fn failed(error: &crate::error::AppError) -> Self {
        Self {
            status: MutationLookupStatus::Failed,
            response: None,
            error: Some(MutationFailure {
                code: error.code().to_string(),
                message: error.to_string(),
            }),
        }
    }

    pub(crate) fn missing() -> Self {
        Self {
            status: MutationLookupStatus::Missing,
            response: None,
            error: None,
        }
    }
}
