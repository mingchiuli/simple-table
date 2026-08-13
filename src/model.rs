use std::collections::HashMap;
use std::rc::Rc;

use crate::protocol::{AppErrorDto, LocalDocumentSummary, SheetImageDto};
use dioxus::prelude::*;
use serde::Deserialize;
use serde_json::Value;

use crate::ports::editor::EditorPort;
use crate::ports::file::FilePort;

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
    #[serde(default)]
    pub column_widths: HashMap<usize, u32>,
    #[serde(default)]
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
    pub formula_status: FormulaStatusView,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorStateView {
    pub can_undo: bool,
    pub can_redo: bool,
    pub is_dirty: bool,
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
    pub cells: Vec<CellView>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellView {
    pub row: usize,
    pub col: usize,
    pub value: Value,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CellPresentation {
    pub display_text: String,
    pub edit_text: String,
    pub formula_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorMutationView {
    #[serde(deserialize_with = "deserialize_u64_string")]
    pub revision: u64,
    pub editor_state: EditorStateView,
    #[serde(default)]
    pub patches: Vec<EditorPatchView>,
    pub sheet_extents: Option<Vec<SheetExtentView>>,
    #[serde(default)]
    pub formula_status: FormulaStatusView,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum EditorPatchView {
    #[serde(rename = "Cells")]
    Cells {
        #[serde(rename = "changes")]
        _changes: Vec<Value>,
    },
    #[serde(rename = "Layout")]
    Layout {
        #[serde(rename = "patch")]
        _patch: Value,
    },
    #[serde(rename = "SheetInserted")]
    SheetInserted {
        #[serde(rename = "patch")]
        _patch: Value,
    },
    #[serde(rename = "SheetDeleted")]
    SheetDeleted {
        #[serde(rename = "patch")]
        _patch: Value,
    },
    #[serde(rename = "SheetInvalidated")]
    SheetInvalidated { patch: SheetPatchView },
    #[serde(rename = "SheetsReplaced")]
    SheetsReplaced {
        #[serde(rename = "patch")]
        _patch: Value,
    },
    #[serde(rename = "RowInserted")]
    RowInserted { patch: SheetPatchView },
    #[serde(rename = "RowDeleted")]
    RowDeleted { patch: SheetPatchView },
    #[serde(rename = "ColumnInserted")]
    ColumnInserted { patch: SheetPatchView },
    #[serde(rename = "ColumnDeleted")]
    ColumnDeleted { patch: SheetPatchView },
    #[serde(rename = "ImageUpserted")]
    ImageUpserted { patch: SheetPatchView },
    #[serde(rename = "ImageDeleted")]
    ImageDeleted { patch: SheetPatchView },
    #[serde(rename = "ResyncRequired")]
    ResyncRequired {
        #[serde(rename = "patch")]
        _patch: Value,
    },
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetPatchView {
    pub sheet_index: usize,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SheetViewport {
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

impl Default for SheetViewport {
    fn default() -> Self {
        Self {
            row_start: 0,
            row_end: 50,
            col_start: 0,
            col_end: 20,
        }
    }
}

#[derive(Clone, Copy)]
pub struct EditorStore {
    pub document: Signal<Option<Rc<OpenDocumentView>>>,
    pub region: Signal<Option<SheetRegionView>>,
    pub active_sheet: Signal<usize>,
    pub selected_cell: Signal<(usize, usize)>,
    pub formula_text: Signal<String>,
    pub busy: Signal<bool>,
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
    pub viewport: Signal<SheetViewport>,
    pub grid_scroll_request: Signal<Option<GridScrollRequest>>,
    pub region_generation: Signal<u64>,
}

type CellCoordinates = (usize, usize, usize);
pub(crate) type PendingCellEdits = HashMap<CellCoordinates, (u64, String)>;

impl EditorStore {
    pub fn new() -> Self {
        Self {
            document: Signal::new(None),
            region: Signal::new(None),
            active_sheet: Signal::new(0),
            selected_cell: Signal::new((0, 0)),
            formula_text: Signal::new(String::new()),
            busy: Signal::new(false),
            error: Signal::new(None),
            status: Signal::new("Ready".to_string()),
            search: Signal::new(None),
            search_open: Signal::new(false),
            local_documents: Signal::new(Vec::new()),
            images: Signal::new(Rc::new(Vec::new())),
            image_assets: Signal::new(Rc::new(HashMap::new())),
            selected_image: Signal::new(None),
            edit_generation: Signal::new(0),
            pending_edits: Signal::new(HashMap::new()),
            viewport: Signal::new(SheetViewport::default()),
            grid_scroll_request: Signal::new(None),
            region_generation: Signal::new(0),
        }
    }

    pub fn active_sheet(&self) -> usize {
        (self.active_sheet)()
    }

    pub fn selected_cell(&self) -> (usize, usize) {
        (self.selected_cell)()
    }

    pub fn busy(&self) -> bool {
        (self.busy)()
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
        self.busy.set(false);
    }

    pub fn accept_document(mut self, mut document: OpenDocumentView) {
        self.region.set(document.initial_region.take());
        self.active_sheet.set(0);
        self.selected_cell.set((0, 0));
        self.formula_text.set(String::new());
        self.pending_edits.write().clear();
        self.search.set(None);
        self.search_open.set(false);
        self.edit_generation
            .set(self.edit_generation().wrapping_add(1));
        self.viewport.set(SheetViewport::default());
        self.grid_scroll_request.set(None);
        let region_generation = (*self.region_generation.read()).wrapping_add(1);
        self.region_generation.set(region_generation);
        self.document.set(Some(Rc::new(document)));
        self.images.set(Rc::new(Vec::new()));
        self.image_assets.set(Rc::new(HashMap::new()));
        self.selected_image.set(None);
        self.busy.set(false);
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

    pub fn accept_mutation(mut self, mutation: &EditorMutationView) {
        if let Some(document) = self.document.write().as_mut().map(Rc::make_mut) {
            document.editor_session.revision = mutation.revision;
            document.editor_session.editor_state = mutation.editor_state.clone();
            document.editor_session.formula_status = mutation.formula_status.clone();
            if let Some(extents) = &mutation.sheet_extents {
                for (sheet, extent) in document.document.sheets.iter_mut().zip(extents) {
                    sheet.extent = *extent;
                }
            }
        }
        self.status.set("Changes saved in memory".to_string());
    }

    pub fn cell_presentation_map(
        &self,
        sheet_index: usize,
    ) -> HashMap<(usize, usize), CellPresentation> {
        let mut cells: HashMap<(usize, usize), CellPresentation> = self
            .region
            .read()
            .as_ref()
            .map(|region| {
                region
                    .cells
                    .iter()
                    .map(|cell| ((cell.row, cell.col), cell_presentation(&cell.value)))
                    .collect()
            })
            .unwrap_or_default();
        for ((pending_sheet, row, col), (_, value)) in self.pending_edits.read().iter() {
            if *pending_sheet == sheet_index {
                cells.insert(
                    (*row, *col),
                    CellPresentation {
                        display_text: value.clone(),
                        edit_text: value.clone(),
                        formula_error: None,
                    },
                );
            }
        }
        cells
    }

    pub fn cell_edit_text(&self, sheet_index: usize, row: usize, col: usize) -> String {
        self.cell_presentation_map(sheet_index)
            .get(&(row, col))
            .map(|cell| cell.edit_text.clone())
            .unwrap_or_default()
    }
}

pub struct AppPorts {
    pub editor: Rc<dyn EditorPort>,
    pub files: Rc<dyn FilePort>,
    pub operations: Rc<futures::lock::Mutex<()>>,
}

impl Clone for AppPorts {
    fn clone(&self) -> Self {
        Self {
            editor: Rc::clone(&self.editor),
            files: Rc::clone(&self.files),
            operations: Rc::clone(&self.operations),
        }
    }
}

pub fn cell_presentation(value: &Value) -> CellPresentation {
    let raw_text = raw_value_text(value.get("raw"));
    let display_text = value
        .get("display")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| raw_text.clone());
    let edit_text = value
        .get("formula")
        .and_then(|formula| formula.get("formula"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or(raw_text);
    let formula_error = value
        .get("formula")
        .and_then(|formula| formula.get("error"))
        .and_then(Value::as_str)
        .map(str::to_string);

    CellPresentation {
        display_text,
        edit_text,
        formula_error,
    }
}

fn raw_value_text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(value)) => value.clone(),
        Some(value) => value.to_string(),
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

pub fn request_id(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn formula_cells_display_result_and_edit_source() {
        let presentation = cell_presentation(&json!({
            "raw": 3,
            "display": "3",
            "formula": {
                "formula": "=SUM(A1:A2)",
                "cachedValue": 3
            }
        }));

        assert_eq!(presentation.display_text, "3");
        assert_eq!(presentation.edit_text, "=SUM(A1:A2)");
        assert_eq!(presentation.formula_error, None);
    }

    #[test]
    fn formatted_values_keep_raw_edit_text() {
        let presentation = cell_presentation(&json!({
            "raw": 12.5,
            "display": "$12.50"
        }));

        assert_eq!(presentation.display_text, "$12.50");
        assert_eq!(presentation.edit_text, "12.5");
    }

    #[test]
    fn formula_errors_are_exposed_for_cell_styling() {
        let presentation = cell_presentation(&json!({
            "raw": null,
            "display": "#VALUE!",
            "formula": {
                "formula": "=UNKNOWN(A1)",
                "cachedValue": null,
                "error": "Unknown function UNKNOWN"
            }
        }));

        assert_eq!(presentation.display_text, "#VALUE!");
        assert_eq!(presentation.edit_text, "=UNKNOWN(A1)");
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

    fn issue(sheet_index: usize, row: usize) -> FormulaIssueView {
        FormulaIssueView {
            sheet_index,
            row,
            col: 0,
            kind: FormulaIssueKindView::InvalidFormula,
            message: "Invalid formula".to_string(),
        }
    }
}
