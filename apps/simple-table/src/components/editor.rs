mod dialogs;
mod formula;
mod image_tools;
mod sheet_strip;
mod table_tools;
mod titlebar;
mod toolbar;

use std::rc::Rc;

use dioxus::prelude::*;
use dioxus::router::Navigator;
use simple_table_components::icons::{House, Sheet};
use simple_table_components::{Button, ButtonSize};

#[cfg(feature = "mobile")]
use self::dialogs::MobileSaveDialog;
use self::dialogs::UnsavedChangesDialog;
use self::formula::FormulaBar;
use self::sheet_strip::SheetStrip;
pub(super) use self::table_tools::TableDataTools;
use self::titlebar::EditorTitlebar;
use self::toolbar::EditorToolbar;
use super::grid::SpreadsheetGrid;
use super::search::SearchPanel;
use crate::Route;
use crate::actions;
use crate::model::{AppPorts, EditorStore};

#[derive(Clone, PartialEq)]
pub(super) enum PendingEditorAction {
    Close,
    New,
    Open { name: String, bytes: Vec<u8> },
}

#[derive(Clone, Copy)]
pub(super) struct EditorUiState {
    pub pending_action: Signal<Option<PendingEditorAction>>,
    #[cfg(feature = "mobile")]
    pub pending_save_name: Signal<Option<String>>,
}

#[component]
pub fn EditorView() -> Element {
    let store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let navigator = use_navigator();
    let pending_action = use_signal(|| None::<PendingEditorAction>);
    #[cfg(feature = "mobile")]
    let pending_save_name = use_signal(|| None::<String>);
    use_context_provider(|| EditorUiState {
        pending_action,
        #[cfg(feature = "mobile")]
        pending_save_name,
    });

    let dirty_window = Rc::clone(&ports.window);
    use_effect(move || {
        sync_unsaved_changes_warning(store, dirty_window.as_ref());
    });
    let cleanup_window = Rc::clone(&ports.window);
    use_drop(move || cleanup_window.set_unsaved_changes_warning(false));

    let document = store.document.read().clone();
    let Some(document) = document else {
        return rsx! {
            main { class: "editor-empty",
                div { class: "brand-mark large", Sheet { size: 28 } }
                h1 { "No workbook open" }
                p { "Create a workbook or open an Excel or CSV file." }
                Button {
                    class: "primary-command",
                    size: ButtonSize::Lg,
                    onclick: move |_| {
                        navigator.replace(Route::Home {});
                    },
                    House { size: 18 }
                    "Back to files"
                }
            }
        };
    };
    let document_id = document.editor_session.document_id;
    #[cfg(feature = "mobile")]
    let mobile_save_dialog = rsx! { MobileSaveDialog {} };
    #[cfg(not(feature = "mobile"))]
    let mobile_save_dialog = rsx! {};

    rsx! {
        main { class: if store.search_open() { "editor-shell search-visible" } else { "editor-shell" },
            EditorTitlebar {}
            EditorToolbar {}
            FormulaBar {}
            div { id: "spreadsheet-panel", class: "editor-workspace",
                SpreadsheetGrid { key: "{document_id}" }
                SearchPanel {}
            }
            SheetStrip {}
            UnsavedChangesDialog {}
            {mobile_save_dialog}
        }
    }
}

pub(super) fn blocked_tooltip(enabled: bool, reasons: &[String], fallback: &str) -> Option<String> {
    (!enabled).then(|| {
        reasons
            .first()
            .cloned()
            .unwrap_or_else(|| fallback.to_string())
    })
}

pub(super) fn has_unsaved_changes(store: EditorStore) -> bool {
    store
        .document
        .read()
        .as_ref()
        .is_some_and(|document| document.editor_session.editor_state.is_dirty)
        || !store.pending_edits.read().is_empty()
}

fn sync_unsaved_changes_warning(store: EditorStore, window: &dyn crate::ports::window::WindowPort) {
    window.set_unsaved_changes_warning(has_unsaved_changes(store));
}

pub(super) fn request_editor_action(
    action: PendingEditorAction,
    mut pending_action: Signal<Option<PendingEditorAction>>,
    store: EditorStore,
    ports: Rc<AppPorts>,
    navigator: Navigator,
) {
    if has_unsaved_changes(store) {
        pending_action.set(Some(action));
    } else {
        spawn(run_editor_action(action, store, ports, navigator));
    }
}

pub(super) async fn run_editor_action(
    action: PendingEditorAction,
    store: EditorStore,
    ports: Rc<AppPorts>,
    navigator: Navigator,
) {
    match action {
        PendingEditorAction::Close => {
            if actions::close_document(store, ports).await {
                navigator.replace(Route::Home {});
            }
        }
        PendingEditorAction::New => {
            if actions::new_document(store, ports).await {
                navigator.replace(Route::Table {});
            }
        }
        PendingEditorAction::Open { name, bytes } => {
            if actions::open_bytes(store, ports, name, bytes).await {
                navigator.replace(Route::Table {});
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::model::{
        DocumentManifestView, EditorSessionView, EditorStateView, OpenDocumentView,
        SheetExtentView, SheetLayoutView, SheetManifestView, use_editor_store,
    };
    use crate::ports::window::WindowPort;

    thread_local! {
        static TEST_STORE: RefCell<Option<EditorStore>> = const { RefCell::new(None) };
    }

    #[component]
    fn StoreHarness() -> Element {
        let store = use_editor_store();
        TEST_STORE.with(|slot| slot.replace(Some(store)));
        rsx! {}
    }

    #[derive(Default)]
    struct RecordingWindowPort {
        warnings: RefCell<Vec<bool>>,
    }

    impl WindowPort for RecordingWindowPort {
        fn open_external(&self, _url: &str) {}

        fn set_unsaved_changes_warning(&self, enabled: bool) {
            self.warnings.borrow_mut().push(enabled);
        }
    }

    fn document(dirty: bool) -> OpenDocumentView {
        OpenDocumentView {
            document: DocumentManifestView {
                path: "/tmp/book.xlsx".to_string(),
                file_name: "book.xlsx".to_string(),
                sheets: vec![SheetManifestView {
                    name: "Sheet1".to_string(),
                    extent: SheetExtentView {
                        row_count: 1,
                        column_count: 1,
                    },
                    layout: Rc::new(SheetLayoutView::default()),
                }],
            },
            editor_session: EditorSessionView {
                document_id: 7,
                revision: 1,
                editor_state: EditorStateView {
                    can_undo: false,
                    can_redo: false,
                    is_dirty: dirty,
                    history: Default::default(),
                },
                capabilities: Default::default(),
                formula_status: Default::default(),
                filters: Vec::new(),
            },
            initial_region: None,
        }
    }

    #[test]
    fn dirty_warning_combines_backend_and_pending_edits() {
        let mut dom = VirtualDom::new(StoreHarness);
        dom.rebuild_in_place();
        let mut store = TEST_STORE.with(|slot| slot.borrow().expect("captured editor store"));
        let window = RecordingWindowPort::default();

        store.document.set(Some(Rc::new(document(false))));
        sync_unsaved_changes_warning(store, &window);
        store.document.set(Some(Rc::new(document(true))));
        sync_unsaved_changes_warning(store, &window);
        store.document.set(Some(Rc::new(document(false))));
        store
            .pending_edits
            .write()
            .insert((0, 0, 0), (1, Rc::<str>::from("pending")));
        sync_unsaved_changes_warning(store, &window);
        store.pending_edits.write().clear();
        sync_unsaved_changes_warning(store, &window);

        assert_eq!(
            window.warnings.borrow().as_slice(),
            &[false, true, true, false]
        );
        TEST_STORE.with(|slot| slot.replace(None));
    }
}
