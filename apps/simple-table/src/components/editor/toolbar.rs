use std::rc::Rc;

use dioxus::prelude::*;
use dioxus::router::use_navigator;
use simple_table_components::icons::{
    Columns3, Download, FilePlus, FolderOpen, Redo2, Rows3, Save, Search, Trash2, Undo2,
};
#[cfg(not(feature = "mobile"))]
use simple_table_components::{ContentSide, Label, Tooltip, TooltipContent, TooltipTrigger};
use simple_table_components::{Toolbar, ToolbarSeparator};

use super::image_tools::{ImageTools, InsertImageTool};
use super::table_tools::{DimensionControls, TableDataTools};
use super::{EditorUiState, PendingEditorAction, blocked_tooltip, request_editor_action};
use crate::actions;
use crate::model::{AppPorts, EditorStore};
use crate::ui::ToolbarIconButton;

#[component]
pub(super) fn EditorToolbar() -> Element {
    let mut store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let navigator = use_navigator();
    let ui_state = use_context::<EditorUiState>();
    #[cfg(feature = "mobile")]
    let mut pending_save_name = ui_state.pending_save_name;
    let document = store.document.read().clone();
    let Some(document) = document else {
        return rsx! {};
    };
    let document_id = document.editor_session.document_id;
    let editor_state = document.editor_session.editor_state.clone();
    let sheet_index = store
        .active_sheet()
        .min(document.document.sheets.len().saturating_sub(1));
    let capabilities = document.editor_session.capabilities.clone();
    let sheet_capabilities = capabilities
        .sheets
        .get(sheet_index)
        .cloned()
        .unwrap_or_default();
    let save_tooltip = blocked_tooltip(
        capabilities.save.can_native_save,
        &capabilities.save.blocked_save_reasons,
        "This workbook cannot be saved without losing unsupported content",
    );
    let row_structure_tooltip = blocked_tooltip(
        sheet_capabilities.can_insert_delete_rows,
        &sheet_capabilities.blocked_row_structure_reasons,
        "Row structure changes are unavailable for this sheet",
    );
    let column_structure_tooltip = blocked_tooltip(
        sheet_capabilities.can_insert_delete_columns,
        &sheet_capabilities.blocked_column_structure_reasons,
        "Column structure changes are unavailable for this sheet",
    );
    let resize_tooltip = blocked_tooltip(
        sheet_capabilities.can_resize_rows_columns,
        &sheet_capabilities.blocked_resize_reasons,
        "Row and column resizing is unavailable for this sheet",
    );
    let image_tooltip = blocked_tooltip(
        capabilities.rich.images.can_insert,
        &capabilities.rich.images.blocked_reasons,
        "Images cannot be inserted into this workbook",
    );
    let selection = *store.selection.read();
    let selected = (selection.row, selection.col);
    #[cfg(feature = "mobile")]
    let file_name = document.document.file_name.clone();
    #[cfg(feature = "mobile")]
    let needs_save_name = document.document.path.is_empty();

    rsx! {
        Toolbar { class: "editor-toolbar", aria_label: "Workbook tools",
            ToolbarIconButton {
                index: 0usize,
                label: "New workbook",
                disabled: store.busy(),
                on_click: {
                    let ports = Rc::clone(&ports);
                    move |_| {
                        request_editor_action(
                            PendingEditorAction::New,
                            ui_state.pending_action,
                            store,
                            Rc::clone(&ports),
                            navigator,
                        );
                    }
                },
                FilePlus { size: 18 }
            }
            OpenDocumentTool {}
            ToolbarSeparator {}
            ToolbarIconButton {
                index: 2usize,
                label: "Save",
                tooltip: save_tooltip.clone(),
                disabled: store.busy() || !capabilities.save.can_native_save,
                on_click: {
                    let ports = Rc::clone(&ports);
                    #[cfg(feature = "mobile")]
                    let suggested_name = file_name;
                    move |_| {
                        #[cfg(feature = "mobile")]
                        if needs_save_name {
                            pending_save_name.set(Some(suggested_name.clone()));
                            return;
                        }
                        let ports = Rc::clone(&ports);
                        spawn(async move { actions::save_local(store, ports).await });
                    }
                },
                Save { size: 18 }
            }
            ToolbarIconButton {
                index: 3usize,
                label: "Download a copy",
                tooltip: save_tooltip,
                disabled: store.busy() || !capabilities.save.can_native_save,
                on_click: {
                    let ports = Rc::clone(&ports);
                    move |_| {
                        let ports = Rc::clone(&ports);
                        spawn(async move { actions::download_copy(store, ports).await });
                    }
                },
                Download { size: 18 }
            }
            ToolbarSeparator {}
            ToolbarIconButton {
                index: 4usize,
                label: "Undo",
                disabled: store.busy() || !editor_state.can_undo,
                on_click: {
                    let ports = Rc::clone(&ports);
                    move |_| {
                        let ports = Rc::clone(&ports);
                        spawn(async move { actions::undo(store, ports).await });
                    }
                },
                Undo2 { size: 18 }
            }
            ToolbarIconButton {
                index: 5usize,
                label: "Redo",
                disabled: store.busy() || !editor_state.can_redo,
                on_click: {
                    let ports = Rc::clone(&ports);
                    move |_| {
                        let ports = Rc::clone(&ports);
                        spawn(async move { actions::redo(store, ports).await });
                    }
                },
                Redo2 { size: 18 }
            }
            if editor_state.history.is_truncated {
                span {
                    class: "history-status",
                    title: editor_state.history.reason.as_deref().unwrap_or("Older undo history was discarded"),
                    "History limited"
                }
            }
            ToolbarSeparator {}
            StructureButton {
                index: 6,
                title: "Insert row",
                icon: rsx! { Rows3 { size: 18 } },
                request: EditorRequestFactory::AddRow,
                sheet_index,
                selected,
                enabled: sheet_capabilities.can_insert_delete_rows,
                blocked_reason: row_structure_tooltip.clone(),
            }
            StructureButton {
                index: 7,
                title: "Delete row",
                icon: rsx! { Trash2 { size: 17 } },
                request: EditorRequestFactory::DeleteRow,
                sheet_index,
                selected,
                enabled: sheet_capabilities.can_insert_delete_rows,
                blocked_reason: row_structure_tooltip,
            }
            StructureButton {
                index: 8,
                title: "Insert column",
                icon: rsx! { Columns3 { size: 18 } },
                request: EditorRequestFactory::AddColumn,
                sheet_index,
                selected,
                enabled: sheet_capabilities.can_insert_delete_columns,
                blocked_reason: column_structure_tooltip.clone(),
            }
            StructureButton {
                index: 9,
                title: "Delete column",
                icon: rsx! { Trash2 { size: 17 } },
                request: EditorRequestFactory::DeleteColumn,
                sheet_index,
                selected,
                enabled: sheet_capabilities.can_insert_delete_columns,
                blocked_reason: column_structure_tooltip,
            }
            DimensionControls {
                sheet_index,
                selected,
                width: document.document.sheets[sheet_index]
                    .layout.column_widths.get(&selected.1).copied().unwrap_or(120),
                height: document.document.sheets[sheet_index]
                    .layout.row_heights.get(&selected.0).copied().unwrap_or(30),
                enabled: sheet_capabilities.can_resize_rows_columns,
                blocked_reason: resize_tooltip,
            }
            ToolbarSeparator {}
            TableDataTools { document_id, sheet_index, selected }
            ToolbarSeparator {}
            InsertImageTool {
                sheet_index,
                selected,
                enabled: capabilities.rich.images.can_insert,
                blocked_reason: image_tooltip,
            }
            if let Some(image_id) = store.selected_image.read().clone()
                && let Some(image) = store.images.read().iter().find(|image| image.id == image_id).cloned()
            {
                ImageTools { image, sheet_index, selected }
            }
            div { class: "toolbar-fill" }
            ToolbarIconButton {
                index: 11usize,
                label: "Find in workbook",
                active: store.search_open(),
                on_click: move |_| {
                    let open = store.search_open();
                    store.search_open.set(!open);
                },
                Search { size: 18 }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum EditorRequestFactory {
    AddRow,
    DeleteRow,
    AddColumn,
    DeleteColumn,
}

#[derive(Props, Clone, PartialEq)]
struct StructureButtonProps {
    index: usize,
    title: &'static str,
    icon: Element,
    request: EditorRequestFactory,
    sheet_index: usize,
    selected: (usize, usize),
    enabled: bool,
    blocked_reason: Option<String>,
}

#[component]
fn StructureButton(props: StructureButtonProps) -> Element {
    let store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    rsx! {
        ToolbarIconButton {
            index: props.index,
            label: props.title,
            tooltip: props.blocked_reason.clone(),
            disabled: store.busy() || !props.enabled,
            on_click: {
                let ports = Rc::clone(&ports);
                move |_| {
                    let intent = match props.request {
                        EditorRequestFactory::AddRow => actions::MutationIntent::AddRow {
                            sheet_index: props.sheet_index,
                            row_index: props.selected.0 + 1,
                        },
                        EditorRequestFactory::DeleteRow => actions::MutationIntent::DeleteRow {
                            sheet_index: props.sheet_index,
                            row_index: props.selected.0,
                        },
                        EditorRequestFactory::AddColumn => actions::MutationIntent::AddColumn {
                            sheet_index: props.sheet_index,
                            col_index: props.selected.1 + 1,
                        },
                        EditorRequestFactory::DeleteColumn => actions::MutationIntent::DeleteColumn {
                            sheet_index: props.sheet_index,
                            col_index: props.selected.1,
                        },
                    };
                    let ports = Rc::clone(&ports);
                    spawn(async move { actions::run_mutation(store, ports, intent).await });
                }
            },
            {props.icon}
        }
    }
}

#[component]
fn OpenDocumentTool() -> Element {
    let store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let navigator = use_navigator();
    let ui_state = use_context::<EditorUiState>();

    #[cfg(feature = "mobile")]
    return rsx! {
        ToolbarIconButton {
            index: 1usize,
            label: "Open workbook",
            disabled: store.busy(),
            on_click: {
                let ports = Rc::clone(&ports);
                move |_| {
                    let ports = Rc::clone(&ports);
                    spawn(async move {
                        match ports
                            .files
                            .pick_file(crate::ports::file::MobileFileKind::Workbook)
                            .await
                        {
                            Ok(Some(file)) => {
                                request_editor_action(
                                    PendingEditorAction::Open {
                                        name: file.name,
                                        bytes: file.bytes,
                                    },
                                    ui_state.pending_action,
                                    store,
                                    ports,
                                    navigator,
                                );
                            }
                            Ok(None) => {}
                            Err(error) => store.set_error(error),
                        }
                    });
                }
            },
            FolderOpen { size: 18 }
        }
    };

    #[cfg(not(feature = "mobile"))]
    rsx! {
        Tooltip { disabled: store.busy(),
            TooltipTrigger {
                Label {
                    class: "tool-button file-tool",
                    html_for: "editor-open-workbook",
                    aria_label: "Open workbook",
                    FolderOpen { size: 18 }
                    input {
                        id: "editor-open-workbook",
                        class: "visually-hidden",
                        r#type: "file",
                        disabled: store.busy(),
                        accept: ".xlsx,.xlsm,.csv,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet,application/vnd.ms-excel.sheet.macroEnabled.12,text/csv",
                        onchange: {
                            let ports = Rc::clone(&ports);
                            move |event: Event<FormData>| {
                                let Some(file) = event.files().into_iter().next() else { return; };
                                let ports = Rc::clone(&ports);
                                spawn(async move {
                                    let name = file.name();
                                    match file.read_bytes().await {
                                        Ok(bytes) => {
                                            request_editor_action(
                                                PendingEditorAction::Open {
                                                    name,
                                                    bytes: bytes.to_vec(),
                                                },
                                                ui_state.pending_action,
                                                store,
                                                ports,
                                                navigator,
                                            );
                                        }
                                        Err(error) => store.set_error(crate::protocol::AppErrorDto {
                                            code: "read_error".to_string(),
                                            message: error.to_string(),
                                        }),
                                    }
                                });
                            }
                        }
                    }
                }
            }
            TooltipContent { side: ContentSide::Bottom, "Open workbook" }
        }
    }
}
