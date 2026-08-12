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
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorStateView {
    pub can_undo: bool,
    pub can_redo: bool,
    pub is_dirty: bool,
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

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorMutationView {
    #[serde(deserialize_with = "deserialize_u64_string")]
    pub revision: u64,
    pub editor_state: EditorStateView,
    pub sheet_extents: Option<Vec<SheetExtentView>>,
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
}

type CellCoordinates = (usize, usize, usize);
type PendingCellEdits = HashMap<CellCoordinates, (u64, String)>;

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
        self.active_sheet.set(
            self.active_sheet()
                .min(document.document.sheets.len().saturating_sub(1)),
        );
        self.document.set(Some(Rc::new(document)));
        self.images.set(Rc::new(Vec::new()));
        self.image_assets.set(Rc::new(HashMap::new()));
        self.selected_image.set(None);
        self.busy.set(false);
        self.error.set(None);
        self.status.set("Ready".to_string());
    }

    pub fn accept_mutation(mut self, mutation: &EditorMutationView) {
        if let Some(document) = self.document.write().as_mut().map(Rc::make_mut) {
            document.editor_session.revision = mutation.revision;
            document.editor_session.editor_state = mutation.editor_state.clone();
            if let Some(extents) = &mutation.sheet_extents {
                for (sheet, extent) in document.document.sheets.iter_mut().zip(extents) {
                    sheet.extent = *extent;
                }
            }
        }
        self.status.set("Changes saved in memory".to_string());
    }

    pub fn display_cell_map(&self, sheet_index: usize) -> HashMap<(usize, usize), String> {
        let mut cells: HashMap<(usize, usize), String> = self
            .region
            .read()
            .as_ref()
            .map(|region| {
                region
                    .cells
                    .iter()
                    .map(|cell| ((cell.row, cell.col), cell_value_text(&cell.value)))
                    .collect()
            })
            .unwrap_or_default();
        for ((pending_sheet, row, col), (_, value)) in self.pending_edits.read().iter() {
            if *pending_sheet == sheet_index {
                cells.insert((*row, *col), value.clone());
            }
        }
        cells
    }
}

pub struct AppPorts {
    pub editor: Rc<dyn EditorPort>,
    pub files: Rc<dyn FilePort>,
}

impl Clone for AppPorts {
    fn clone(&self) -> Self {
        Self {
            editor: Rc::clone(&self.editor),
            files: Rc::clone(&self.files),
        }
    }
}

pub fn cell_value_text(value: &Value) -> String {
    value
        .get("formula")
        .and_then(|formula| formula.get("formula"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| value.get("display").and_then(serde_json::Value::as_str))
        .or_else(|| value.get("raw").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .or_else(|| value.get("raw").map(ToString::to_string))
        .unwrap_or_default()
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
