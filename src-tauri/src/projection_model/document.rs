use std::collections::HashMap;

use crate::document::capabilities::WorkbookCapabilities;
use crate::document::region_metadata_index::{DocumentRegion, DocumentRegionMetadata};
use crate::document_data::{CellFormat, CellStyle, SheetExtent};
use crate::domain::CellValue;
use crate::projection_model::EditorSessionSnapshot;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SheetLayoutSnapshot {
    pub column_widths: HashMap<usize, u32>,
    pub row_heights: HashMap<usize, u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SheetManifestSnapshot {
    pub name: String,
    pub extent: SheetExtent,
    pub layout: SheetLayoutSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DocumentManifestSnapshot {
    pub path: String,
    pub file_name: String,
    pub sheets: Vec<SheetManifestSnapshot>,
}

#[derive(Clone, Debug)]
pub(crate) struct ProjectedCellChange {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub value: CellValue,
    pub display: Option<String>,
    pub format: Option<CellFormat>,
    pub style: Option<CellStyle>,
}

impl ProjectedCellChange {
    pub(crate) fn new(sheet_index: usize, row: usize, col: usize, value: CellValue) -> Self {
        Self {
            sheet_index,
            row,
            col,
            value,
            display: None,
            format: None,
            style: None,
        }
    }

    pub(crate) fn with_display_projection(
        mut self,
        display: String,
        format: Option<CellFormat>,
        style: Option<CellStyle>,
    ) -> Self {
        self.display = Some(display);
        self.format = format;
        self.style = style;
        self
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SheetRegionSnapshot {
    pub document_id: u64,
    pub revision: u64,
    pub region: DocumentRegion,
    pub cells: Vec<ProjectedCellChange>,
    pub merge_anchor_cells: Vec<ProjectedCellChange>,
    pub metadata: DocumentRegionMetadata,
}

#[derive(Clone, Debug)]
pub(crate) struct OpenDocumentSnapshot {
    pub document: DocumentManifestSnapshot,
    pub editor_session: EditorSessionSnapshot,
    pub initial_region: Option<SheetRegionSnapshot>,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedOpenDocument {
    pub token: String,
    pub preview: OpenDocumentSnapshot,
}

#[derive(Clone, Debug)]
pub(crate) struct SavedDocumentIdentity {
    pub path: String,
    pub file_name: String,
}

#[derive(Clone, Debug)]
pub(crate) struct SavedDocumentOutcome {
    pub document: Option<DocumentManifestSnapshot>,
    pub identity: Option<SavedDocumentIdentity>,
    pub editor_session: EditorSessionSnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileOperationKind {
    Open,
    Save,
    Close,
    Export,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileOperationReceipt {
    pub kind: FileOperationKind,
    pub document_id: u64,
    pub revision: u64,
    pub path: String,
    pub file_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileOperationLookupStatus {
    Pending,
    Completed,
    Failed,
    Cancelled,
    Missing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileOperationFailure {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileOperationLookup {
    pub status: FileOperationLookupStatus,
    pub receipt: Option<FileOperationReceipt>,
    pub error: Option<FileOperationFailure>,
}

impl FileOperationLookup {
    pub(crate) fn pending() -> Self {
        Self {
            status: FileOperationLookupStatus::Pending,
            receipt: None,
            error: None,
        }
    }

    pub(crate) fn completed(receipt: FileOperationReceipt) -> Self {
        Self {
            status: FileOperationLookupStatus::Completed,
            receipt: Some(receipt),
            error: None,
        }
    }

    pub(crate) fn failed(error: &crate::error::AppError) -> Self {
        Self {
            status: FileOperationLookupStatus::Failed,
            receipt: None,
            error: Some(FileOperationFailure {
                code: error.code().to_string(),
                message: error.to_string(),
            }),
        }
    }

    pub(crate) fn cancelled() -> Self {
        Self {
            status: FileOperationLookupStatus::Cancelled,
            receipt: None,
            error: None,
        }
    }

    pub(crate) fn missing() -> Self {
        Self {
            status: FileOperationLookupStatus::Missing,
            receipt: None,
            error: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SpreadsheetFormatOptions {
    pub default_extension: String,
    pub supported_extensions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DocumentCapabilities {
    pub source_format: String,
    pub can_save_original: bool,
    pub native_save_format: Option<String>,
    pub export_formats: Vec<String>,
    pub native_save_extension: Option<String>,
    pub export_extension: String,
    pub requires_save_as_for_native_save: bool,
    pub workbook: WorkbookCapabilities,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeSavePlan {
    pub can_save: bool,
    pub requires_save_as: bool,
    pub native_save_extension: Option<String>,
    pub default_extension: String,
    pub blocked_reason: Option<String>,
    pub capabilities: DocumentCapabilities,
}
