pub(crate) mod region_cache;

use std::collections::HashMap;
use std::rc::Rc;

use crate::protocol::{AppErrorDto, HistoryStatus, SheetImageDto, WorkbookCapabilities};
use dioxus::prelude::*;
use serde::Deserialize;

use crate::ports::editor::EditorPort;
use crate::ports::file::FilePort;
#[cfg(feature = "mobile")]
use crate::ports::recovery::RecoveryPort;
#[cfg(not(feature = "mobile"))]
use crate::ports::workspace::LocalWorkspacePort;

pub use region_cache::{DocumentRevision, RegionCache, RegionTileKey};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenDocumentView {
    pub document: DocumentManifestView,
    pub editor_session: EditorSessionView,
    pub initial_region: Option<SheetRegionView>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentManifestView {
    #[serde(default)]
    pub path: String,
    pub file_name: String,
    pub sheets: Vec<SheetManifestView>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetManifestView {
    pub name: String,
    pub extent: SheetExtentView,
    #[serde(default)]
    pub layout: Rc<SheetLayoutView>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SheetLayoutView {
    #[serde(default, deserialize_with = "deserialize_index_map")]
    pub column_widths: HashMap<usize, u32>,
    #[serde(default, deserialize_with = "deserialize_index_map")]
    pub row_heights: HashMap<usize, u32>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetExtentView {
    pub row_count: usize,
    pub column_count: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSessionView {
    #[serde(deserialize_with = "deserialize_u64_string")]
    pub document_id: u64,
    #[serde(deserialize_with = "deserialize_u64_string")]
    pub revision: u64,
    pub editor_state: EditorStateView,
    #[serde(default)]
    pub capabilities: WorkbookCapabilities,
    #[serde(default)]
    pub formula_status: FormulaStatusView,
    #[serde(default)]
    pub filters: Vec<SheetFilterView>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CellRangeView {
    pub start_row: usize,
    pub end_row: usize,
    pub start_col: usize,
    pub end_col: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FilterOperatorView {
    Equals,
    NotEquals,
    Contains,
    Blank,
    NotBlank,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FilterConditionView {
    pub col: usize,
    pub operator: FilterOperatorView,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetFilterView {
    pub sheet_index: usize,
    pub range: CellRangeView,
    pub conditions: Vec<FilterConditionView>,
    pub hidden_rows: Vec<usize>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorStateView {
    pub can_undo: bool,
    pub can_redo: bool,
    pub is_dirty: bool,
    #[serde(default)]
    pub history: HistoryStatus,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FormulaDiagnosticsView {
    pub invalid_formula_count: usize,
    pub volatile_formula_count: usize,
    pub unsupported_dependency_count: usize,
    pub large_range_dependency_count: usize,
    pub skipped_reference_rewrite_count: usize,
    #[serde(default)]
    pub issues: Vec<FormulaIssueView>,
}

impl FormulaDiagnosticsView {
    pub fn total_count(&self) -> usize {
        self.invalid_formula_count
            .saturating_add(self.volatile_formula_count)
            .saturating_add(self.unsupported_dependency_count)
            .saturating_add(self.large_range_dependency_count)
            .saturating_add(self.skipped_reference_rewrite_count)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FormulaIssueView {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub kind: FormulaIssueKindView,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FormulaIssueKindView {
    InvalidFormula,
    VolatileFormula,
    UnsupportedDependency,
    LargeRangeDependency,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum FormulaStatusView {
    Ready {
        #[serde(default)]
        diagnostics: FormulaDiagnosticsView,
    },
    Degraded {
        message: String,
        #[serde(default)]
        diagnostics: FormulaDiagnosticsView,
    },
}

impl Default for FormulaStatusView {
    fn default() -> Self {
        Self::Ready {
            diagnostics: FormulaDiagnosticsView::default(),
        }
    }
}

impl FormulaStatusView {
    pub fn diagnostics(&self) -> &FormulaDiagnosticsView {
        match self {
            Self::Ready { diagnostics } | Self::Degraded { diagnostics, .. } => diagnostics,
        }
    }

    pub fn degraded_message(&self) -> Option<&str> {
        match self {
            Self::Ready { .. } => None,
            Self::Degraded { message, .. } => Some(message),
        }
    }

    pub fn sample_issues(&self, active_sheet: usize, maximum: usize) -> Vec<FormulaIssueView> {
        let issues = &self.diagnostics().issues;
        issues
            .iter()
            .filter(|issue| issue.sheet_index == active_sheet)
            .chain(
                issues
                    .iter()
                    .filter(|issue| issue.sheet_index != active_sheet),
            )
            .take(maximum)
            .cloned()
            .collect()
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetRegionView {
    #[serde(deserialize_with = "deserialize_u64_string")]
    pub document_id: u64,
    #[serde(deserialize_with = "deserialize_u64_string")]
    pub revision: u64,
    pub region: SheetRegionBoundsView,
    pub cells: Vec<CellView>,
    #[serde(default)]
    pub merge_anchor_cells: Vec<CellView>,
    #[serde(default)]
    pub metadata: SheetRegionMetadataView,
    #[serde(default)]
    pub wire_bytes: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetRowsRegionView {
    pub regions: Vec<SheetRegionView>,
    #[serde(default, rename = "wireBytes")]
    pub _wire_bytes: usize,
}

#[cfg(test)]
impl SheetRegionView {
    pub fn merge_range_at(&self, row: usize, col: usize) -> Option<MergeRangeView> {
        self.normalized_merge_ranges()
            .into_iter()
            .find(|merge| merge.contains(row, col))
    }

    pub fn normalize_cell(&self, row: usize, col: usize) -> (usize, usize) {
        self.merge_range_at(row, col)
            .map_or((row, col), |merge| merge.anchor())
    }

    pub fn normalized_merge_ranges(&self) -> Vec<MergeRangeView> {
        let mut merges = self
            .metadata
            .merges
            .iter()
            .copied()
            .filter(|merge| merge.is_valid())
            .collect::<Vec<_>>();
        merges.sort_unstable_by_key(MergeRangeView::sort_key);
        let mut accepted = Vec::with_capacity(merges.len());
        for merge in merges {
            if accepted
                .iter()
                .any(|existing: &MergeRangeView| existing.overlaps(merge))
            {
                continue;
            }
            accepted.push(merge);
        }
        accepted
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Hash, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetRegionBoundsView {
    pub sheet_index: usize,
    pub row_start: usize,
    pub row_end: usize,
    pub col_start: usize,
    pub col_end: usize,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetRegionMetadataView {
    #[serde(default)]
    pub merges: Vec<MergeRangeView>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MergeRangeView {
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
}

impl MergeRangeView {
    pub fn anchor(self) -> (usize, usize) {
        (self.start_row, self.start_col)
    }

    pub fn row_span(self) -> usize {
        self.end_row
            .saturating_sub(self.start_row)
            .saturating_add(1)
    }

    pub fn col_span(self) -> usize {
        self.end_col
            .saturating_sub(self.start_col)
            .saturating_add(1)
    }

    pub fn contains(self, row: usize, col: usize) -> bool {
        row >= self.start_row && row <= self.end_row && col >= self.start_col && col <= self.end_col
    }

    pub fn intersects(
        self,
        row_start: usize,
        row_end: usize,
        col_start: usize,
        col_end: usize,
    ) -> bool {
        self.start_row < row_end
            && self.end_row >= row_start
            && self.start_col < col_end
            && self.end_col >= col_start
    }

    fn overlaps(self, other: Self) -> bool {
        self.start_row <= other.end_row
            && self.end_row >= other.start_row
            && self.start_col <= other.end_col
            && self.end_col >= other.start_col
    }

    #[cfg(test)]
    fn is_valid(self) -> bool {
        self.start_row <= self.end_row
            && self.start_col <= self.end_col
            && (self.start_row != self.end_row || self.start_col != self.end_col)
    }

    #[cfg(test)]
    fn sort_key(&self) -> (usize, usize, usize, usize) {
        (self.start_row, self.start_col, self.end_row, self.end_col)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellView {
    #[serde(default)]
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub display_text: String,
    pub edit_text: String,
    pub formula_error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CellPresentation {
    pub display_text: Rc<str>,
    pub edit_text: Rc<str>,
    pub formula_error: Option<Rc<str>>,
}

#[derive(Clone, Debug)]
pub struct EditorMutationView {
    pub document_id: u64,
    pub revision: u64,
    pub editor_state: EditorStateView,
    pub capabilities: WorkbookCapabilities,
    pub patches: Vec<EditorPatchView>,
    pub sheet_extents: Option<Vec<SheetExtentView>>,
    pub formula_status: FormulaStatusView,
    pub filters: Vec<SheetFilterView>,
}

#[derive(Clone, Debug)]
pub enum EditorPatchView {
    Cells { changes: Vec<CellView> },
    Layout { patch: LayoutPatchView },
    SheetInserted,
    SheetDeleted,
    SheetInvalidated { patch: SheetPatchView },
    SheetsReplaced,
    RowInserted { patch: SheetPatchView },
    RowDeleted { patch: SheetPatchView },
    ColumnInserted { patch: SheetPatchView },
    ColumnDeleted { patch: SheetPatchView },
    ImageUpserted { patch: SheetPatchView },
    ImageDeleted { patch: SheetPatchView },
    ResyncRequired,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetPatchView {
    pub sheet_index: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutPatchView {
    pub sheet_index: usize,
    #[serde(default, deserialize_with = "deserialize_index_map")]
    pub column_widths: HashMap<usize, Option<u32>>,
    #[serde(default, deserialize_with = "deserialize_index_map")]
    pub row_heights: HashMap<usize, Option<u32>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchView {
    pub results: Vec<SearchResultView>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultView {
    pub sheet_index: usize,
    pub sheet_name: String,
    pub row: usize,
    pub col: usize,
    pub value: String,
    pub cell_position: String,
}

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub struct GridRenderWindow {
    pub sheet_index: usize,
    pub row_start: usize,
    pub row_end: usize,
    pub col_start: usize,
    pub col_end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridScrollRequest {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub focus: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedDocumentView {
    pub document: Option<DocumentManifestView>,
    pub identity: Option<SavedDocumentIdentityView>,
    pub editor_session: EditorSessionView,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedDocumentIdentityView {
    pub path: String,
    pub file_name: String,
}

impl From<crate::protocol::OpenDocumentResponse> for OpenDocumentView {
    fn from(value: crate::protocol::OpenDocumentResponse) -> Self {
        Self {
            document: value.document.into(),
            editor_session: value.editor_session.into(),
            initial_region: value.initial_region.map(Into::into),
        }
    }
}

impl From<crate::protocol::DocumentManifest> for DocumentManifestView {
    fn from(value: crate::protocol::DocumentManifest) -> Self {
        Self {
            path: value.path,
            file_name: value.file_name,
            sheets: value.sheets.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<crate::protocol::SheetManifest> for SheetManifestView {
    fn from(value: crate::protocol::SheetManifest) -> Self {
        Self {
            name: value.name,
            extent: value.extent.into(),
            layout: Rc::new(value.layout.into()),
        }
    }
}

impl From<crate::protocol::SheetLayoutProjection> for SheetLayoutView {
    fn from(value: crate::protocol::SheetLayoutProjection) -> Self {
        Self {
            column_widths: value.column_widths,
            row_heights: value.row_heights,
        }
    }
}

impl From<crate::protocol::SheetExtent> for SheetExtentView {
    fn from(value: crate::protocol::SheetExtent) -> Self {
        Self {
            row_count: value.row_count,
            column_count: value.column_count,
        }
    }
}

impl From<crate::protocol::EditorSessionInfo> for EditorSessionView {
    fn from(value: crate::protocol::EditorSessionInfo) -> Self {
        Self {
            document_id: value.document_id,
            revision: value.revision,
            editor_state: value.editor_state.into(),
            capabilities: value.capabilities,
            formula_status: value.formula_status.into(),
            filters: value.filters.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<crate::protocol::EditorStateInfo> for EditorStateView {
    fn from(value: crate::protocol::EditorStateInfo) -> Self {
        Self {
            can_undo: value.can_undo,
            can_redo: value.can_redo,
            is_dirty: value.is_dirty,
            history: value.history,
        }
    }
}

impl From<crate::protocol::SheetFilterInfo> for SheetFilterView {
    fn from(value: crate::protocol::SheetFilterInfo) -> Self {
        Self {
            sheet_index: value.sheet_index,
            range: CellRangeView {
                start_row: value.range.start_row,
                end_row: value.range.end_row,
                start_col: value.range.start_col,
                end_col: value.range.end_col,
            },
            conditions: value.conditions.into_iter().map(Into::into).collect(),
            hidden_rows: value.hidden_rows,
        }
    }
}

impl From<crate::protocol::FilterConditionInfo> for FilterConditionView {
    fn from(value: crate::protocol::FilterConditionInfo) -> Self {
        Self {
            col: value.col,
            operator: match value.operator {
                crate::protocol::FilterOperatorInfo::Equals => FilterOperatorView::Equals,
                crate::protocol::FilterOperatorInfo::NotEquals => FilterOperatorView::NotEquals,
                crate::protocol::FilterOperatorInfo::Contains => FilterOperatorView::Contains,
                crate::protocol::FilterOperatorInfo::Blank => FilterOperatorView::Blank,
                crate::protocol::FilterOperatorInfo::NotBlank => FilterOperatorView::NotBlank,
            },
            value: value.value,
        }
    }
}

impl From<crate::protocol::FormulaStatus> for FormulaStatusView {
    fn from(value: crate::protocol::FormulaStatus) -> Self {
        match value {
            crate::protocol::FormulaStatus::Ready { diagnostics } => Self::Ready {
                diagnostics: diagnostics.into(),
            },
            crate::protocol::FormulaStatus::Degraded {
                message,
                diagnostics,
            } => Self::Degraded {
                message,
                diagnostics: diagnostics.into(),
            },
        }
    }
}

impl From<crate::protocol::FormulaDiagnostics> for FormulaDiagnosticsView {
    fn from(value: crate::protocol::FormulaDiagnostics) -> Self {
        Self {
            invalid_formula_count: value.invalid_formula_count,
            volatile_formula_count: value.volatile_formula_count,
            unsupported_dependency_count: value.unsupported_dependency_count,
            large_range_dependency_count: value.large_range_dependency_count,
            skipped_reference_rewrite_count: value.skipped_reference_rewrite_count,
            issues: value.issues.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<crate::protocol::FormulaIssue> for FormulaIssueView {
    fn from(value: crate::protocol::FormulaIssue) -> Self {
        Self {
            sheet_index: value.sheet_index,
            row: value.row,
            col: value.col,
            kind: match value.kind {
                crate::protocol::FormulaIssueKind::InvalidFormula => {
                    FormulaIssueKindView::InvalidFormula
                }
                crate::protocol::FormulaIssueKind::VolatileFormula => {
                    FormulaIssueKindView::VolatileFormula
                }
                crate::protocol::FormulaIssueKind::UnsupportedDependency => {
                    FormulaIssueKindView::UnsupportedDependency
                }
                crate::protocol::FormulaIssueKind::LargeRangeDependency => {
                    FormulaIssueKindView::LargeRangeDependency
                }
            },
            message: value.message,
        }
    }
}

impl From<crate::protocol::SheetRegionProjectionResponse> for SheetRegionView {
    fn from(value: crate::protocol::SheetRegionProjectionResponse) -> Self {
        Self {
            document_id: value.document_id,
            revision: value.revision,
            region: value.region.into(),
            cells: value.cells.into_iter().map(Into::into).collect(),
            merge_anchor_cells: value
                .merge_anchor_cells
                .into_iter()
                .map(Into::into)
                .collect(),
            metadata: value.metadata.into(),
            wire_bytes: value.wire_bytes,
        }
    }
}

impl From<crate::protocol::SheetRowsRegionProjectionResponse> for SheetRowsRegionView {
    fn from(value: crate::protocol::SheetRowsRegionProjectionResponse) -> Self {
        Self {
            regions: value.regions.into_iter().map(Into::into).collect(),
            _wire_bytes: value.wire_bytes,
        }
    }
}

impl From<crate::protocol::SheetRegion> for SheetRegionBoundsView {
    fn from(value: crate::protocol::SheetRegion) -> Self {
        Self {
            sheet_index: value.sheet_index,
            row_start: value.row_start,
            row_end: value.row_end,
            col_start: value.col_start,
            col_end: value.col_end,
        }
    }
}

impl From<crate::protocol::SheetRegionMetadata> for SheetRegionMetadataView {
    fn from(value: crate::protocol::SheetRegionMetadata) -> Self {
        Self {
            merges: value.merges.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<crate::protocol::MergeRange> for MergeRangeView {
    fn from(value: crate::protocol::MergeRange) -> Self {
        Self {
            start_row: value.start_row as usize,
            start_col: usize::from(value.start_col),
            end_row: value.end_row as usize,
            end_col: usize::from(value.end_col),
        }
    }
}

impl From<crate::protocol::SheetCellChange> for CellView {
    fn from(value: crate::protocol::SheetCellChange) -> Self {
        Self {
            sheet_index: value.sheet_index,
            row: value.row,
            col: value.col,
            display_text: value.display_text,
            edit_text: value.edit_text,
            formula_error: value.formula_error,
        }
    }
}

impl From<crate::protocol::EditorMutationResponse> for EditorMutationView {
    fn from(value: crate::protocol::EditorMutationResponse) -> Self {
        Self {
            document_id: value.document_id,
            revision: value.revision,
            editor_state: value.editor_state.into(),
            capabilities: value.capabilities,
            patches: value.patches.into_iter().map(Into::into).collect(),
            sheet_extents: value
                .sheet_extents
                .map(|extents| extents.into_iter().map(Into::into).collect()),
            formula_status: value.formula_status.into(),
            filters: value.filters.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<crate::protocol::EditorPatch> for EditorPatchView {
    fn from(value: crate::protocol::EditorPatch) -> Self {
        use crate::protocol::EditorPatch;

        match value {
            EditorPatch::Cells { changes } => Self::Cells {
                changes: changes.into_iter().map(Into::into).collect(),
            },
            EditorPatch::Layout { patch } => Self::Layout {
                patch: LayoutPatchView {
                    sheet_index: patch.sheet_index,
                    column_widths: patch.column_widths,
                    row_heights: patch.row_heights,
                },
            },
            EditorPatch::SheetInserted { .. } => Self::SheetInserted,
            EditorPatch::SheetDeleted { .. } => Self::SheetDeleted,
            EditorPatch::SheetInvalidated { patch } => Self::SheetInvalidated {
                patch: SheetPatchView {
                    sheet_index: patch.sheet_index,
                },
            },
            EditorPatch::SheetsReplaced { .. } => Self::SheetsReplaced,
            EditorPatch::RowInserted { patch } => Self::RowInserted {
                patch: SheetPatchView {
                    sheet_index: patch.sheet_index,
                },
            },
            EditorPatch::RowDeleted { patch } => Self::RowDeleted {
                patch: SheetPatchView {
                    sheet_index: patch.sheet_index,
                },
            },
            EditorPatch::ColumnInserted { patch } => Self::ColumnInserted {
                patch: SheetPatchView {
                    sheet_index: patch.sheet_index,
                },
            },
            EditorPatch::ColumnDeleted { patch } => Self::ColumnDeleted {
                patch: SheetPatchView {
                    sheet_index: patch.sheet_index,
                },
            },
            EditorPatch::ImageUpserted { patch } => Self::ImageUpserted {
                patch: SheetPatchView {
                    sheet_index: patch.sheet_index,
                },
            },
            EditorPatch::ImageDeleted { patch } => Self::ImageDeleted {
                patch: SheetPatchView {
                    sheet_index: patch.sheet_index,
                },
            },
            EditorPatch::ResyncRequired { .. } => Self::ResyncRequired,
        }
    }
}

impl From<crate::protocol::SearchResponse> for SearchView {
    fn from(value: crate::protocol::SearchResponse) -> Self {
        Self {
            results: value.results.into_iter().map(Into::into).collect(),
            truncated: value.truncated,
        }
    }
}

impl From<crate::protocol::SearchResult> for SearchResultView {
    fn from(value: crate::protocol::SearchResult) -> Self {
        Self {
            sheet_index: value.sheet_index,
            sheet_name: value.sheet_name,
            row: value.row,
            col: value.col,
            value: value.value,
            cell_position: value.cell_position,
        }
    }
}

impl From<crate::protocol::SavedDocumentResponse> for SavedDocumentView {
    fn from(value: crate::protocol::SavedDocumentResponse) -> Self {
        Self {
            document: value.document.map(Into::into),
            identity: value.identity.map(|identity| SavedDocumentIdentityView {
                path: identity.path,
                file_name: identity.file_name,
            }),
            editor_session: value.editor_session.into(),
        }
    }
}

impl GridRenderWindow {
    pub fn bounds(self) -> SheetRegionBoundsView {
        SheetRegionBoundsView {
            sheet_index: self.sheet_index,
            row_start: self.row_start,
            row_end: self.row_end,
            col_start: self.col_start,
            col_end: self.col_end,
        }
    }

    pub fn clamped(self, sheet_index: usize, row_count: usize, column_count: usize) -> Self {
        let row_count = row_count.max(1);
        let column_count = column_count.max(1);
        if self.sheet_index != sheet_index
            || self.row_end <= self.row_start
            || self.col_end <= self.col_start
        {
            return Self {
                sheet_index,
                row_start: 0,
                row_end: 1,
                col_start: 0,
                col_end: 1,
            };
        }

        let row_start = self.row_start.min(row_count - 1);
        let col_start = self.col_start.min(column_count - 1);
        Self {
            sheet_index,
            row_start,
            row_end: self.row_end.min(row_count).max(row_start + 1),
            col_start,
            col_end: self.col_end.min(column_count).max(col_start + 1),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GridSelection {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub merge: Option<MergeRangeView>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalDocumentSummary {
    pub id: String,
    pub name: String,
    pub updated_at_ms: u64,
    pub has_recovery: bool,
}

#[derive(Clone, Copy)]
pub struct EditorStore {
    pub document: Signal<Option<Rc<OpenDocumentView>>>,
    pub region_cache: Signal<RegionCache>,
    pub active_sheet: Signal<usize>,
    pub selection: Signal<GridSelection>,
    pub formula_text: Signal<String>,
    operation: Signal<OperationState>,
    pub error: Signal<Option<AppErrorDto>>,
    pub status: Signal<String>,
    pub search: Signal<Option<SearchView>>,
    pub search_open: Signal<bool>,
    pub local_documents: Signal<Vec<LocalDocumentSummary>>,
    pub images: Signal<Rc<Vec<SheetImageDto>>>,
    pub image_assets: Signal<Rc<HashMap<String, Rc<str>>>>,
    pub selected_image: Signal<Option<String>>,
    pub edit_generation: Signal<u64>,
    pub pending_edits: Signal<PendingCellEdits>,
    pub render_window: Signal<GridRenderWindow>,
    pub grid_scroll_request: Signal<Option<GridScrollRequest>>,
}

type CellCoordinates = (usize, usize, usize);
pub(crate) type PendingCellEdits = HashMap<CellCoordinates, (u64, Rc<str>)>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct OperationState {
    next_id: u64,
    active_id: Option<u64>,
}

impl OperationState {
    fn begin(&mut self) -> u64 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.active_id = Some(self.next_id);
        self.next_id
    }

    fn finish(&mut self, id: u64) {
        if self.active_id == Some(id) {
            self.active_id = None;
        }
    }

    fn is_active(self) -> bool {
        self.active_id.is_some()
    }
}

#[must_use = "the operation guard must live until the asynchronous action finishes"]
pub(crate) struct OperationGuard {
    operation: Signal<OperationState>,
    id: u64,
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        if let Ok(mut operation) = self.operation.try_write() {
            operation.finish(self.id);
        }
    }
}

pub(crate) fn use_editor_store() -> EditorStore {
    EditorStore {
        document: use_signal(|| None),
        region_cache: use_signal(RegionCache::default),
        active_sheet: use_signal(|| 0),
        selection: use_signal(GridSelection::default),
        formula_text: use_signal(String::new),
        operation: use_signal(OperationState::default),
        error: use_signal(|| None),
        status: use_signal(|| "Ready".to_string()),
        search: use_signal(|| None),
        search_open: use_signal(|| false),
        local_documents: use_signal(Vec::new),
        images: use_signal(|| Rc::new(Vec::new())),
        image_assets: use_signal(|| Rc::new(HashMap::new())),
        selected_image: use_signal(|| None),
        edit_generation: use_signal(|| 0),
        pending_edits: use_signal(HashMap::new),
        render_window: use_signal(GridRenderWindow::default),
        grid_scroll_request: use_signal(|| None),
    }
}

impl EditorStore {
    pub fn active_sheet(&self) -> usize {
        (self.active_sheet)()
    }

    pub fn selected_cell(&self) -> (usize, usize) {
        let selection = (self.selection)();
        (selection.row, selection.col)
    }

    pub fn select_cell(mut self, sheet_index: usize, row: usize, col: usize) {
        let merge = self
            .region_cache
            .peek()
            .merge_range_at(sheet_index, row, col);
        let (row, col) = merge.map_or((row, col), MergeRangeView::anchor);
        self.selection.set(GridSelection {
            sheet_index,
            row,
            col,
            merge,
        });
    }

    pub fn busy(&self) -> bool {
        self.operation.read().is_active()
    }

    pub(crate) fn begin_operation(mut self, status: &str) -> OperationGuard {
        let id = self.operation.write().begin();
        self.error.set(None);
        self.status.set(status.to_string());
        OperationGuard {
            operation: self.operation,
            id,
        }
    }

    pub fn search_open(&self) -> bool {
        (self.search_open)()
    }

    pub fn edit_generation(&self) -> u64 {
        (self.edit_generation)()
    }

    pub fn set_error(mut self, error: AppErrorDto) {
        self.status.set("Action failed".to_string());
        self.error.set(Some(error));
    }

    pub fn accept_document(mut self, mut document: OpenDocumentView) {
        let initial_region = document.initial_region.take();
        let identity = DocumentRevision {
            document_id: document.editor_session.document_id,
            revision: document.editor_session.revision,
        };
        let mut cache = RegionCache::new(identity);
        if let Some(region) = initial_region {
            cache.insert_region(region.region, vec![region]);
        }
        self.region_cache.set(cache);
        self.active_sheet.set(0);
        self.selection.set(GridSelection::default());
        self.formula_text.set(String::new());
        self.pending_edits.write().clear();
        self.search.set(None);
        self.search_open.set(false);
        self.edit_generation
            .set(self.edit_generation().wrapping_add(1));
        self.render_window.set(GridRenderWindow::default());
        self.grid_scroll_request.set(None);
        self.document.set(Some(Rc::new(document)));
        self.images.set(Rc::new(Vec::new()));
        self.image_assets.set(Rc::new(HashMap::new()));
        self.selected_image.set(None);
        self.error.set(None);
        self.status.set("Ready".to_string());
    }

    pub fn refresh_document(mut self, mut document: OpenDocumentView) {
        document.initial_region = None;
        self.active_sheet.set(
            self.active_sheet()
                .min(document.document.sheets.len().saturating_sub(1)),
        );
        self.document.set(Some(Rc::new(document)));
    }

    pub fn accept_mutation(mut self, mutation: EditorMutationView) {
        let EditorMutationView {
            document_id,
            revision,
            editor_state,
            capabilities,
            patches,
            sheet_extents,
            formula_status,
            filters,
        } = mutation;
        if let Some(document) = self.document.write().as_mut().map(Rc::make_mut) {
            document.editor_session.revision = revision;
            document.editor_session.editor_state = editor_state;
            document.editor_session.capabilities = capabilities;
            document.editor_session.formula_status = formula_status;
            document.editor_session.filters = filters;
            if let Some(extents) = &sheet_extents {
                for (sheet, extent) in document.document.sheets.iter_mut().zip(extents) {
                    sheet.extent = *extent;
                }
            }
            for patch in &patches {
                let EditorPatchView::Layout { patch } = patch else {
                    continue;
                };
                let Some(sheet) = document.document.sheets.get_mut(patch.sheet_index) else {
                    continue;
                };
                let layout = Rc::make_mut(&mut sheet.layout);
                apply_layout_changes(&mut layout.column_widths, &patch.column_widths);
                apply_layout_changes(&mut layout.row_heights, &patch.row_heights);
            }
        }
        self.region_cache
            .write()
            .apply_mutation_parts(document_id, revision, &patches);
        self.status.set("Changes saved in memory".to_string());
    }

    pub fn sheet_filter(&self, sheet_index: usize) -> Option<SheetFilterView> {
        self.document
            .peek()
            .as_ref()?
            .editor_session
            .filters
            .iter()
            .find(|filter| filter.sheet_index == sheet_index)
            .cloned()
    }

    pub fn visible_rows(&self, sheet_index: usize, row_count: usize) -> Vec<usize> {
        let hidden = self
            .sheet_filter(sheet_index)
            .map(|filter| {
                filter
                    .hidden_rows
                    .into_iter()
                    .collect::<std::collections::HashSet<_>>()
            })
            .unwrap_or_default();
        (0..row_count).filter(|row| !hidden.contains(row)).collect()
    }

    pub fn cell_presentation_map(
        &self,
        sheet_index: usize,
    ) -> HashMap<(usize, usize), CellPresentation> {
        let mut cells = self.region_cache.peek().projection(sheet_index, None).cells;
        for ((pending_sheet, row, col), (_, value)) in self.pending_edits.read().iter() {
            if *pending_sheet == sheet_index {
                cells.insert(
                    (*row, *col),
                    CellPresentation {
                        display_text: Rc::clone(value),
                        edit_text: Rc::clone(value),
                        formula_error: None,
                    },
                );
            }
        }
        cells
    }

    pub fn cell_edit_text(&self, sheet_index: usize, row: usize, col: usize) -> String {
        let (row, col) = self.normalize_cell(sheet_index, row, col);
        self.cell_presentation_map(sheet_index)
            .get(&(row, col))
            .map(|cell| cell.edit_text.to_string())
            .unwrap_or_default()
    }

    pub fn merge_range_at(
        &self,
        sheet_index: usize,
        row: usize,
        col: usize,
    ) -> Option<MergeRangeView> {
        self.region_cache
            .peek()
            .merge_range_at(sheet_index, row, col)
    }

    pub fn normalize_cell(&self, sheet_index: usize, row: usize, col: usize) -> (usize, usize) {
        self.merge_range_at(sheet_index, row, col)
            .map_or((row, col), MergeRangeView::anchor)
    }
}

fn apply_layout_changes(target: &mut HashMap<usize, u32>, changes: &HashMap<usize, Option<u32>>) {
    for (&index, &size) in changes {
        if let Some(size) = size {
            target.insert(index, size);
        } else {
            target.remove(&index);
        }
    }
}

pub struct AppPorts {
    pub editor: Rc<dyn EditorPort>,
    pub regions: crate::actions::RegionLoader,
    pub files: Rc<dyn FilePort>,
    #[cfg(not(feature = "mobile"))]
    pub workspace: Rc<dyn LocalWorkspacePort>,
    #[cfg(feature = "mobile")]
    pub recovery: Rc<dyn RecoveryPort>,
    pub operations: Rc<futures::lock::Mutex<()>>,
}

impl Clone for AppPorts {
    fn clone(&self) -> Self {
        Self {
            editor: Rc::clone(&self.editor),
            regions: self.regions.clone(),
            files: Rc::clone(&self.files),
            #[cfg(not(feature = "mobile"))]
            workspace: Rc::clone(&self.workspace),
            #[cfg(feature = "mobile")]
            recovery: Rc::clone(&self.recovery),
            operations: Rc::clone(&self.operations),
        }
    }
}

pub fn cell_presentation(value: &CellView) -> CellPresentation {
    CellPresentation {
        display_text: Rc::from(value.display_text.as_str()),
        edit_text: Rc::from(value.edit_text.as_str()),
        formula_error: value.formula_error.as_deref().map(Rc::from),
    }
}

fn deserialize_u64_string<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrNumber {
        String(String),
        Number(u64),
    }

    match StringOrNumber::deserialize(deserializer)? {
        StringOrNumber::String(value) => value.parse().map_err(serde::de::Error::custom),
        StringOrNumber::Number(value) => Ok(value),
    }
}

fn deserialize_index_map<'de, D, V>(deserializer: D) -> Result<HashMap<usize, V>, D::Error>
where
    D: serde::Deserializer<'de>,
    V: Deserialize<'de>,
{
    HashMap::<String, V>::deserialize(deserializer)?
        .into_iter()
        .map(|(index, value)| {
            index
                .parse()
                .map(|index| (index, value))
                .map_err(serde::de::Error::custom)
        })
        .collect()
}

pub fn request_id(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stale_operation_completion_does_not_clear_the_current_operation() {
        let mut operation = OperationState::default();
        let first = operation.begin();
        let second = operation.begin();

        operation.finish(first);

        assert!(operation.is_active());
        assert_eq!(operation.active_id, Some(second));
        operation.finish(second);
        assert!(!operation.is_active());
    }

    #[test]
    fn operation_completion_is_idempotent() {
        let mut operation = OperationState::default();
        let id = operation.begin();

        operation.finish(id);
        operation.finish(id);

        assert!(!operation.is_active());
    }

    #[test]
    fn formula_cells_display_result_and_edit_source() {
        let presentation = cell_presentation(&CellView {
            sheet_index: 0,
            row: 0,
            col: 0,
            display_text: "3".to_string(),
            edit_text: "=SUM(A1:A2)".to_string(),
            formula_error: None,
        });

        assert_eq!(presentation.display_text.as_ref(), "3");
        assert_eq!(presentation.edit_text.as_ref(), "=SUM(A1:A2)");
        assert_eq!(presentation.formula_error, None);
    }

    #[test]
    fn formatted_values_keep_raw_edit_text() {
        let presentation = cell_presentation(&CellView {
            sheet_index: 0,
            row: 0,
            col: 0,
            display_text: "$12.50".to_string(),
            edit_text: "12.5".to_string(),
            formula_error: None,
        });

        assert_eq!(presentation.display_text.as_ref(), "$12.50");
        assert_eq!(presentation.edit_text.as_ref(), "12.5");
    }

    #[test]
    fn index_keyed_layout_maps_deserialize_from_protocol_values() {
        let layout: SheetLayoutView = serde_json::from_value(json!({
            "columnWidths": { "0": 120 },
            "rowHeights": { "2": 30 }
        }))
        .expect("document layout should accept JSON object keys");
        let patch: LayoutPatchView = serde_json::from_value(json!({
            "sheetIndex": 0,
            "columnWidths": { "0": 120 },
            "rowHeights": { "2": null }
        }))
        .expect("layout patch should accept JSON object keys");

        assert_eq!(layout.column_widths.get(&0), Some(&120));
        assert_eq!(layout.row_heights.get(&2), Some(&30));
        assert_eq!(patch.column_widths.get(&0), Some(&Some(120)));
        assert_eq!(patch.row_heights.get(&2), Some(&None));
    }

    #[test]
    fn formula_errors_are_exposed_for_cell_styling() {
        let presentation = cell_presentation(&CellView {
            sheet_index: 0,
            row: 0,
            col: 0,
            display_text: "#VALUE!".to_string(),
            edit_text: "=UNKNOWN(A1)".to_string(),
            formula_error: Some("Unknown function UNKNOWN".to_string()),
        });

        assert_eq!(presentation.display_text.as_ref(), "#VALUE!");
        assert_eq!(presentation.edit_text.as_ref(), "=UNKNOWN(A1)");
        assert_eq!(
            presentation.formula_error.as_deref(),
            Some("Unknown function UNKNOWN")
        );
    }

    #[test]
    fn diagnostic_samples_prioritize_the_active_sheet_and_apply_the_limit() {
        let status = FormulaStatusView::Ready {
            diagnostics: FormulaDiagnosticsView {
                issues: vec![
                    issue(0, 0),
                    issue(1, 1),
                    issue(0, 2),
                    issue(1, 3),
                    issue(2, 4),
                ],
                ..Default::default()
            },
        };

        let samples = status.sample_issues(1, 3);

        assert_eq!(
            samples
                .iter()
                .map(|issue| (issue.sheet_index, issue.row))
                .collect::<Vec<_>>(),
            vec![(1, 1), (1, 3), (0, 0)]
        );
    }

    #[test]
    fn formula_status_deserializes_from_the_protocol_shape() {
        let status: FormulaStatusView = serde_json::from_value(json!({
            "state": "degraded",
            "message": "Formula work limit exceeded",
            "diagnostics": {
                "invalidFormulaCount": 2,
                "volatileFormulaCount": 0,
                "unsupportedDependencyCount": 1,
                "largeRangeDependencyCount": 0,
                "skippedReferenceRewriteCount": 0,
                "issues": [{
                    "sheetIndex": 0,
                    "row": 3,
                    "col": 1,
                    "kind": "invalidFormula",
                    "message": "Invalid formula"
                }]
            }
        }))
        .expect("formula status should match the protocol projection");

        assert_eq!(
            status.degraded_message(),
            Some("Formula work limit exceeded")
        );
        assert_eq!(status.diagnostics().total_count(), 3);
        assert_eq!(status.diagnostics().issues[0].col, 1);
    }

    #[test]
    fn sheet_region_deserializes_merges_and_external_anchor_cells() {
        let region: SheetRegionView = serde_json::from_value(json!({
            "documentId": "7",
            "revision": "3",
            "region": {
                "sheetIndex": 0,
                "rowStart": 8,
                "rowEnd": 12,
                "colStart": 1,
                "colEnd": 3
            },
            "cells": [],
            "mergeAnchorCells": [{
                "sheetIndex": 0,
                "row": 2,
                "col": 1,
                "displayText": "Merged",
                "editText": "Merged",
                "formulaError": null
            }],
            "metadata": {
                "merges": [{
                    "startRow": 2,
                    "startCol": 1,
                    "endRow": 10,
                    "endCol": 2
                }]
            },
            "wireBytes": 256
        }))
        .expect("merged region should match the protocol projection");

        assert_eq!(region.normalize_cell(9, 2), (2, 1));
        assert_eq!(region.merge_anchor_cells.len(), 1);
        assert_eq!(region.merge_anchor_cells[0].row, 2);
        assert!(region.metadata.merges[0].intersects(8, 12, 1, 3));
    }

    #[test]
    fn normalized_merges_ignore_degenerate_and_overlapping_ranges() {
        let region = SheetRegionView {
            document_id: 7,
            revision: 3,
            region: SheetRegionBoundsView::default(),
            cells: Vec::new(),
            merge_anchor_cells: Vec::new(),
            metadata: SheetRegionMetadataView {
                merges: vec![
                    merge(2, 2, 3, 3),
                    merge(0, 0, 1, 1),
                    merge(1, 1, 2, 2),
                    merge(4, 4, 4, 4),
                ],
            },
            wire_bytes: 256,
        };

        assert_eq!(
            region.normalized_merge_ranges(),
            vec![merge(0, 0, 1, 1), merge(2, 2, 3, 3)]
        );
    }

    #[test]
    fn render_window_is_clamped_when_a_mutation_shrinks_the_sheet() {
        let window = GridRenderWindow {
            sheet_index: 0,
            row_start: 0,
            row_end: 5,
            col_start: 0,
            col_end: 5,
        };

        assert_eq!(
            window.clamped(0, 4, 3),
            GridRenderWindow {
                sheet_index: 0,
                row_start: 0,
                row_end: 4,
                col_start: 0,
                col_end: 3,
            }
        );
    }

    #[test]
    fn invalid_render_window_starts_at_the_requested_sheet_origin() {
        assert_eq!(
            GridRenderWindow::default().clamped(2, 10, 10),
            GridRenderWindow {
                sheet_index: 2,
                row_start: 0,
                row_end: 1,
                col_start: 0,
                col_end: 1,
            }
        );
    }

    fn issue(sheet_index: usize, row: usize) -> FormulaIssueView {
        FormulaIssueView {
            sheet_index,
            row,
            col: 0,
            kind: FormulaIssueKindView::InvalidFormula,
            message: "Invalid formula".to_string(),
        }
    }

    fn merge(start_row: usize, start_col: usize, end_row: usize, end_col: usize) -> MergeRangeView {
        MergeRangeView {
            start_row,
            start_col,
            end_row,
            end_col,
        }
    }
}
