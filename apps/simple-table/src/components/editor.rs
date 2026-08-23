use std::rc::Rc;

use crate::protocol::{ImageAnchorDto, ImageMarkerDto, SheetImageDto};
use dioxus::prelude::*;
use dioxus::router::Navigator;
use simple_table_components::icons::{
    ArrowLeft, Columns3, Download, ExternalLink, FilePlus, FolderOpen, House, ImagePlus, Move,
    Plus, Redo2, Rows3, Save, Search, Sheet, Sigma, Trash2, Undo2,
};
use simple_table_components::{
    AlertDialog, AlertDialogAction, AlertDialogActions, AlertDialogCancel, AlertDialogDescription,
    AlertDialogTitle, Button, ButtonSize, ButtonVariant, ContentAlign, ContentSide, Input, Label,
    PopoverContent, PopoverRoot, PopoverTrigger, TabList, TabTrigger, Tabs, TabsVariant, Toolbar,
    ToolbarSeparator,
};
#[cfg(not(feature = "mobile"))]
use simple_table_components::{Tooltip, TooltipContent, TooltipTrigger};

use super::grid::{SpreadsheetGrid, column_label};
use super::search::SearchPanel;
use crate::Route;
use crate::actions;
use crate::model::{
    AppPorts, EditorStore, FormulaIssueKindView, FormulaIssueView, FormulaStatusView, request_id,
};
use crate::ports::update::{GitHubUpdatePort, UpdatePort};
use crate::ports::window::{PlatformWindowPort, WindowPort};
use crate::ui::ToolbarIconButton;

#[derive(Clone, PartialEq)]
enum PendingEditorAction {
    Close,
    New,
    Open { name: String, bytes: Vec<u8> },
}

#[component]
pub fn EditorView() -> Element {
    let mut store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let navigator = use_navigator();
    let mut pending_action = use_signal(|| None::<PendingEditorAction>);
    use_effect(move || install_dirty_guard(has_unsaved_changes(store)));
    use_drop(|| install_dirty_guard(false));
    let document = store.document.read().clone();

    if document.is_none() {
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
    }
    let document = document.expect("checked above");
    let document_id = document.editor_session.document_id;
    let revision = document.editor_session.revision;
    let editor_state = document.editor_session.editor_state.clone();
    let sheet_index = store
        .active_sheet()
        .min(document.document.sheets.len().saturating_sub(1));
    let selection = *store.selection.read();
    let selected = (selection.row, selection.col);
    let selected_address = selection.merge.map_or_else(
        || format!("{}{}", column_label(selection.col), selection.row + 1),
        |merge| {
            format!(
                "{}{}:{}{}",
                column_label(merge.start_col),
                merge.start_row + 1,
                column_label(merge.end_col),
                merge.end_row + 1
            )
        },
    );
    let file_name = document.document.file_name.clone();
    let sheet_count = document.document.sheets.len();
    let dirty = editor_state.is_dirty || !store.pending_edits.read().is_empty();
    let back_ports = Rc::clone(&ports);
    let confirmed_action = pending_action.read().clone();

    rsx! {
        main { class: if store.search_open() { "editor-shell search-visible" } else { "editor-shell" },
            header { class: "editor-titlebar",
                Button {
                    class: "icon-button editor-back-button",
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::IconSm,
                    aria_label: "Back to files",
                    disabled: store.busy(),
                    onclick: move |_| {
                        request_editor_action(
                            PendingEditorAction::Close,
                            pending_action,
                            store,
                            Rc::clone(&back_ports),
                            navigator,
                        );
                    },
                    ArrowLeft { size: 19 }
                }
                div { class: "editor-document-title",
                    span { class: "brand-mark compact", Sheet { size: 18 } }
                    span { class: "file-title", "{file_name}" }
                }
                if dirty {
                    span { class: "dirty-indicator", "Unsaved changes" }
                } else {
                    span { class: "saved-indicator", "Saved" }
                }
                div { class: "titlebar-spacer" }
                UpdateButton {}
            }

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
                                pending_action,
                                store,
                                Rc::clone(&ports),
                                navigator,
                            );
                        }
                    },
                    FilePlus { size: 18 }
                }
                OpenDocumentTool { pending_action }
                ToolbarSeparator {}
                ToolbarIconButton {
                    index: 2usize,
                    label: "Save",
                    disabled: store.busy(),
                    on_click: {
                        let ports = Rc::clone(&ports);
                        move |_| {
                            let ports = Rc::clone(&ports);
                            spawn(async move { actions::save_local(store, ports).await });
                        }
                    },
                    Save { size: 18 }
                }
                ToolbarIconButton {
                    index: 3usize,
                    label: "Download a copy",
                    disabled: store.busy(),
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
                ToolbarSeparator {}
                StructureButton {
                    index: 6,
                    title: "Insert row",
                    icon: rsx! { Rows3 { size: 18 } },
                    request: EditorRequestFactory::AddRow,
                    document_id,
                    revision,
                    sheet_index,
                    selected,
                }
                StructureButton {
                    index: 7,
                    title: "Delete row",
                    icon: rsx! { Trash2 { size: 17 } },
                    request: EditorRequestFactory::DeleteRow,
                    document_id,
                    revision,
                    sheet_index,
                    selected,
                }
                StructureButton {
                    index: 8,
                    title: "Insert column",
                    icon: rsx! { Columns3 { size: 18 } },
                    request: EditorRequestFactory::AddColumn,
                    document_id,
                    revision,
                    sheet_index,
                    selected,
                }
                StructureButton {
                    index: 9,
                    title: "Delete column",
                    icon: rsx! { Trash2 { size: 17 } },
                    request: EditorRequestFactory::DeleteColumn,
                    document_id,
                    revision,
                    sheet_index,
                    selected,
                }
                DimensionControls {
                    document_id,
                    revision,
                    sheet_index,
                    selected,
                    width: document.document.sheets[sheet_index]
                        .layout.column_widths.get(&selected.1).copied().unwrap_or(120),
                    height: document.document.sheets[sheet_index]
                        .layout.row_heights.get(&selected.0).copied().unwrap_or(30),
                }
                ToolbarSeparator {}
                InsertImageTool {
                    document_id,
                    revision,
                    sheet_index,
                    selected,
                }
                if let Some(image_id) = store.selected_image.read().clone()
                    && let Some(image) = store.images.read().iter().find(|image| image.id == image_id).cloned()
                {
                    ImageTools {
                        image,
                        document_id,
                        revision,
                        sheet_index,
                        selected,
                    }
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

            div { class: "formula-bar",
                span { class: "cell-address", title: selected_address, "{selected_address}" }
                span { class: "formula-symbol", "fx" }
                Input {
                    aria_label: "Cell value or formula",
                    disabled: store.busy(),
                    value: store.formula_text,
                    oninput: {
                        let ports = Rc::clone(&ports);
                        move |event: Event<FormData>| {
                            let text = event.value();
                            store.formula_text.set(text.clone());
                            actions::queue_cell_edit(
                                store,
                                Rc::clone(&ports),
                                sheet_index,
                                selected.0,
                                selected.1,
                                text,
                            );
                        }
                    }
                }
            }

            div { id: "spreadsheet-panel", class: "editor-workspace",
                SpreadsheetGrid { key: "{document_id}" }
                SearchPanel {}
            }

            div { class: "sheet-strip",
                Tabs {
                    class: "sheet-tabs",
                    variant: TabsVariant::Ghost,
                    value: Some(document.document.sheets[sheet_index].name.clone()),
                    default_value: document.document.sheets[sheet_index].name.clone(),
                    horizontal: true,
                    on_value_change: {
                        let sheet_names = document
                            .document
                            .sheets
                            .iter()
                            .map(|sheet| sheet.name.clone())
                            .collect::<Vec<_>>();
                        let ports = Rc::clone(&ports);
                        move |name: String| {
                            if let Some(index) = sheet_names.iter().position(|sheet| sheet == &name) {
                                let ports = Rc::clone(&ports);
                                spawn(async move { actions::select_sheet(store, ports, index).await });
                            }
                        }
                    },
                    TabList { class: "sheet-tab-list",
                        for (index, sheet) in document.document.sheets.iter().enumerate() {
                            TabTrigger {
                                key: "{sheet.name}-{index}",
                                class: "sheet-tab",
                                value: sheet.name.clone(),
                                index,
                                "{sheet.name}"
                            }
                        }
                    }
                }
                Button {
                    class: "icon-button add-sheet",
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::IconSm,
                    aria_label: "Add sheet",
                    disabled: store.busy(),
                    onclick: {
                        let ports = Rc::clone(&ports);
                        move |_| {
                            let ports = Rc::clone(&ports);
                            spawn(async move {
                                actions::run_mutation(
                                    store,
                                    ports,
                                    crate::protocol::EditorRequest::AddSheet {
                                        request_id: request_id("add-sheet"),
                                        document_id,
                                        base_revision: revision,
                                    },
                                )
                                .await;
                            });
                        }
                    },
                    Plus { size: 17 }
                }
                Button {
                    class: "icon-button",
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::IconSm,
                    aria_label: "Delete current sheet",
                    disabled: store.busy() || sheet_count <= 1,
                    onclick: {
                        let ports = Rc::clone(&ports);
                        move |_| {
                            let ports = Rc::clone(&ports);
                            spawn(async move {
                                actions::run_mutation(
                                    store,
                                    ports,
                                    crate::protocol::EditorRequest::DeleteSheet {
                                        request_id: request_id("delete-sheet"),
                                        document_id,
                                        base_revision: revision,
                                        sheet_index,
                                    },
                                )
                                .await;
                            });
                        }
                    },
                    Trash2 { size: 16 }
                }
                div { class: "sheet-strip-spacer" }
                FormulaStatusPopover {
                    status: document.editor_session.formula_status.clone(),
                    sheet_names: document
                        .document
                        .sheets
                        .iter()
                        .map(|sheet| sheet.name.clone())
                        .collect(),
                    active_sheet: sheet_index,
                }
                span { class: "status-text", "{store.status}" }
            }
            AlertDialog {
                open: Some(pending_action.read().is_some()),
                on_open_change: move |open: bool| {
                    if !open {
                        pending_action.set(None);
                    }
                },
                AlertDialogTitle { "Unsaved changes" }
                AlertDialogDescription {
                    "Discard the current workbook changes and continue?"
                }
                AlertDialogActions {
                    AlertDialogCancel {
                        on_click: move |_| pending_action.set(None),
                        "Cancel"
                    }
                    AlertDialogAction {
                        on_click: {
                            let ports = Rc::clone(&ports);
                            move |_| {
                                let Some(action) = confirmed_action.clone() else {
                                    return;
                                };
                                spawn(run_editor_action(
                                    action,
                                    store,
                                    Rc::clone(&ports),
                                    navigator,
                                ));
                            }
                        },
                        "Discard and continue"
                    }
                }
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
struct FormulaStatusPopoverProps {
    status: FormulaStatusView,
    sheet_names: Vec<String>,
    active_sheet: usize,
}

#[component]
fn FormulaStatusPopover(props: FormulaStatusPopoverProps) -> Element {
    let diagnostics = props.status.diagnostics();
    let total = diagnostics.total_count();
    let invalid_count = diagnostics.invalid_formula_count;
    let volatile_count = diagnostics.volatile_formula_count;
    let unsupported_count = diagnostics.unsupported_dependency_count;
    let large_range_count = diagnostics.large_range_dependency_count;
    let skipped_rewrite_count = diagnostics.skipped_reference_rewrite_count;
    let degraded_message = props.status.degraded_message().map(str::to_string);
    let samples = props.status.sample_issues(props.active_sheet, 5);
    let (state_class, state_label, trigger_label) = if degraded_message.is_some() {
        (
            "degraded",
            "Degraded",
            format!("Formula calculation degraded, {total} diagnostics"),
        )
    } else if total > 0 {
        (
            "warning",
            "Warnings",
            format!("Formula calculation has {total} diagnostics"),
        )
    } else {
        ("ready", "Ready", "Formula calculation ready".to_string())
    };
    let trigger_class = format!("formula-status-trigger {state_class}");

    rsx! {
        PopoverRoot { class: "formula-status", is_modal: false,
            PopoverTrigger {
                class: trigger_class,
                title: trigger_label.clone(),
                aria_label: trigger_label,
                Sigma { size: 15 }
                if total > 0 {
                    span { class: "formula-status-count", "{total}" }
                }
            }
            PopoverContent {
                class: "formula-status-popover",
                side: ContentSide::Top,
                align: ContentAlign::End,
                div { class: "formula-status-heading",
                    h2 { "Formula status" }
                    span { class: "formula-state {state_class}", "{state_label}" }
                }
                if let Some(message) = degraded_message {
                    p { class: "formula-degraded-message", "{message}" }
                } else if total == 0 {
                    p { class: "formula-ready-message", "Formulas are calculating normally." }
                }
                dl { class: "formula-diagnostic-counts",
                    FormulaDiagnosticCount { label: "Invalid", value: invalid_count }
                    FormulaDiagnosticCount { label: "Volatile", value: volatile_count }
                    FormulaDiagnosticCount { label: "Dependencies", value: unsupported_count }
                    FormulaDiagnosticCount { label: "Large ranges", value: large_range_count }
                    FormulaDiagnosticCount { label: "Skipped rewrites", value: skipped_rewrite_count }
                }
                if !samples.is_empty() {
                    div { class: "formula-issues",
                        h3 { "Examples" }
                        ul {
                            for (index, issue) in samples.iter().enumerate() {
                                li { key: "{issue.sheet_index}-{issue.row}-{issue.col}-{index}",
                                    div { class: "formula-issue-meta",
                                        strong {
                                            "{formula_issue_location(issue, &props.sheet_names)}"
                                        }
                                        span { "{formula_issue_kind_label(issue.kind)}" }
                                    }
                                    p { "{issue.message}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
struct FormulaDiagnosticCountProps {
    label: &'static str,
    value: usize,
}

#[component]
fn FormulaDiagnosticCount(props: FormulaDiagnosticCountProps) -> Element {
    rsx! {
        div {
            dt { "{props.label}" }
            dd { "{props.value}" }
        }
    }
}

fn formula_issue_location(issue: &FormulaIssueView, sheet_names: &[String]) -> String {
    let sheet_name = sheet_names
        .get(issue.sheet_index)
        .cloned()
        .unwrap_or_else(|| format!("Sheet {}", issue.sheet_index + 1));
    format!("{sheet_name}!{}{}", column_label(issue.col), issue.row + 1)
}

fn formula_issue_kind_label(kind: FormulaIssueKindView) -> &'static str {
    match kind {
        FormulaIssueKindView::InvalidFormula => "Invalid formula",
        FormulaIssueKindView::VolatileFormula => "Volatile formula",
        FormulaIssueKindView::UnsupportedDependency => "Unsupported dependency",
        FormulaIssueKindView::LargeRangeDependency => "Large range",
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
    document_id: u64,
    revision: u64,
    sheet_index: usize,
    selected: (usize, usize),
}

#[component]
fn StructureButton(props: StructureButtonProps) -> Element {
    let store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    rsx! {
        ToolbarIconButton {
            index: props.index,
            label: props.title,
            disabled: store.busy(),
            on_click: {
                let ports = Rc::clone(&ports);
                move |_| {
                    let request_id = request_id("structure");
                    let request = match props.request {
                        EditorRequestFactory::AddRow => crate::protocol::EditorRequest::AddRow {
                            request_id,
                            document_id: props.document_id,
                            base_revision: props.revision,
                            sheet_index: props.sheet_index,
                            row_index: props.selected.0 + 1,
                        },
                        EditorRequestFactory::DeleteRow => crate::protocol::EditorRequest::DeleteRow {
                            request_id,
                            document_id: props.document_id,
                            base_revision: props.revision,
                            sheet_index: props.sheet_index,
                            row_index: props.selected.0,
                        },
                        EditorRequestFactory::AddColumn => crate::protocol::EditorRequest::AddColumn {
                            request_id,
                            document_id: props.document_id,
                            base_revision: props.revision,
                            sheet_index: props.sheet_index,
                            col_index: props.selected.1 + 1,
                        },
                        EditorRequestFactory::DeleteColumn => crate::protocol::EditorRequest::DeleteColumn {
                            request_id,
                            document_id: props.document_id,
                            base_revision: props.revision,
                            sheet_index: props.sheet_index,
                            col_index: props.selected.1,
                        },
                    };
                    let ports = Rc::clone(&ports);
                    spawn(async move { actions::run_mutation(store, ports, request).await });
                }
            },
            {props.icon}
        }
    }
}

#[derive(Props, Clone, PartialEq)]
struct InsertImageToolProps {
    document_id: u64,
    revision: u64,
    sheet_index: usize,
    selected: (usize, usize),
}

#[component]
fn InsertImageTool(props: InsertImageToolProps) -> Element {
    let store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();

    #[cfg(feature = "mobile")]
    return rsx! {
        ToolbarIconButton {
            index: 10usize,
            label: "Insert image",
            disabled: store.busy(),
            on_click: {
                let ports = Rc::clone(&ports);
                move |_| {
                    let ports = Rc::clone(&ports);
                    spawn(async move {
                        match ports
                            .files
                            .pick_file(crate::ports::file::MobileFileKind::Image)
                            .await
                        {
                            Ok(Some(file)) => {
                                actions::run_mutation(
                                    store,
                                    ports,
                                    crate::protocol::EditorRequest::InsertImage {
                                        request_id: request_id("image"),
                                        document_id: props.document_id,
                                        base_revision: props.revision,
                                        sheet_index: props.sheet_index,
                                        row: props.selected.0 as u32,
                                        col: props.selected.1 as u32,
                                        file_name: file.name,
                                        bytes: file.bytes,
                                    },
                                )
                                .await;
                            }
                            Ok(None) => {}
                            Err(error) => store.set_error(error),
                        }
                    });
                }
            },
            ImagePlus { size: 18 }
        }
    };

    #[cfg(not(feature = "mobile"))]
    rsx! {
        Tooltip { disabled: store.busy(),
            TooltipTrigger {
                Label {
                    class: "tool-button file-tool",
                    html_for: "insert-workbook-image",
                    aria_label: "Insert image",
                    ImagePlus { size: 18 }
                    input {
                        id: "insert-workbook-image",
                        class: "visually-hidden",
                        r#type: "file",
                        disabled: store.busy(),
                        accept: "image/png,image/jpeg",
                        onchange: {
                            let ports = Rc::clone(&ports);
                            move |event: Event<FormData>| {
                                let Some(file) = event.files().into_iter().next() else { return; };
                                let ports = Rc::clone(&ports);
                                spawn(async move {
                                    match file.read_bytes().await {
                                        Ok(bytes) => {
                                            actions::run_mutation(
                                                store,
                                                ports,
                                                crate::protocol::EditorRequest::InsertImage {
                                                    request_id: request_id("image"),
                                                    document_id: props.document_id,
                                                    base_revision: props.revision,
                                                    sheet_index: props.sheet_index,
                                                    row: props.selected.0 as u32,
                                                    col: props.selected.1 as u32,
                                                    file_name: file.name(),
                                                    bytes: bytes.to_vec(),
                                                },
                                            )
                                            .await;
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
            TooltipContent { side: ContentSide::Bottom, "Insert image" }
        }
    }
}

#[component]
fn OpenDocumentTool(mut pending_action: Signal<Option<PendingEditorAction>>) -> Element {
    let store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let navigator = use_navigator();
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
                                    pending_action,
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
                                                pending_action,
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

#[derive(Props, Clone, PartialEq)]
struct DimensionControlsProps {
    document_id: u64,
    revision: u64,
    sheet_index: usize,
    selected: (usize, usize),
    width: u32,
    height: u32,
}

#[component]
fn DimensionControls(props: DimensionControlsProps) -> Element {
    let store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    rsx! {
        div { class: "dimension-controls",
            Label { html_for: "selected-column-width", title: "Selected column width",
                span { "W" }
                Input {
                    id: "selected-column-width",
                    r#type: "number",
                    min: 24,
                    max: 600,
                    value: props.width,
                    aria_label: "Selected column width",
                    onchange: {
                        let ports = Rc::clone(&ports);
                        move |event: Event<FormData>| {
                            let Ok(width) = event.value().parse::<u32>() else { return; };
                            let ports = Rc::clone(&ports);
                            spawn(async move {
                                actions::run_mutation(
                                    store,
                                    ports,
                                    crate::protocol::EditorRequest::SetColumnWidth {
                                        request_id: request_id("column-width"),
                                        document_id: props.document_id,
                                        base_revision: props.revision,
                                        sheet_index: props.sheet_index,
                                        col_index: props.selected.1,
                                        width: Some(width.clamp(24, 600)),
                                    },
                                )
                                .await;
                            });
                        }
                    }
                }
            }
            Label { html_for: "selected-row-height", title: "Selected row height",
                span { "H" }
                Input {
                    id: "selected-row-height",
                    r#type: "number",
                    min: 18,
                    max: 300,
                    value: props.height,
                    aria_label: "Selected row height",
                    onchange: {
                        let ports = Rc::clone(&ports);
                        move |event: Event<FormData>| {
                            let Ok(height) = event.value().parse::<u32>() else { return; };
                            let ports = Rc::clone(&ports);
                            spawn(async move {
                                actions::run_mutation(
                                    store,
                                    ports,
                                    crate::protocol::EditorRequest::SetRowHeight {
                                        request_id: request_id("row-height"),
                                        document_id: props.document_id,
                                        base_revision: props.revision,
                                        sheet_index: props.sheet_index,
                                        row_index: props.selected.0,
                                        height: Some(height.clamp(18, 300)),
                                    },
                                )
                                .await;
                            });
                        }
                    }
                }
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
struct ImageToolsProps {
    image: SheetImageDto,
    document_id: u64,
    revision: u64,
    sheet_index: usize,
    selected: (usize, usize),
}

#[component]
fn ImageTools(props: ImageToolsProps) -> Element {
    const EMU_PER_PIXEL: u32 = 9_525;

    let store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let (from, width_emu, height_emu) = match &props.image.anchor {
        ImageAnchorDto::OneCell {
            from,
            width_emu,
            height_emu,
        } => (from.clone(), *width_emu, *height_emu),
        ImageAnchorDto::TwoCell { from, .. } => (
            from.clone(),
            props.image.intrinsic_width.saturating_mul(EMU_PER_PIXEL),
            props.image.intrinsic_height.saturating_mul(EMU_PER_PIXEL),
        ),
    };
    let width_px = (width_emu / EMU_PER_PIXEL).max(1);
    let height_px = (height_emu / EMU_PER_PIXEL).max(1);

    rsx! {
        div { class: "image-tools",
            Button {
                class: "tool-button",
                variant: ButtonVariant::Ghost,
                size: ButtonSize::IconSm,
                aria_label: "Move image to selected cell",
                onclick: {
                    let ports = Rc::clone(&ports);
                    let image_id = props.image.id.clone();
                    move |_| {
                        let ports = Rc::clone(&ports);
                        let image_id = image_id.clone();
                        spawn(async move {
                            actions::run_mutation(
                                store,
                                ports,
                                crate::protocol::EditorRequest::UpdateImage {
                                    request_id: request_id("move-image"),
                                    document_id: props.document_id,
                                    base_revision: props.revision,
                                    sheet_index: props.sheet_index,
                                    image_id,
                                    anchor: ImageAnchorDto::OneCell {
                                        from: ImageMarkerDto {
                                            row: props.selected.0 as u32,
                                            col: props.selected.1 as u32,
                                            row_offset_emu: 0,
                                            col_offset_emu: 0,
                                        },
                                        width_emu,
                                        height_emu,
                                    },
                                },
                            )
                            .await;
                        });
                    }
                },
                Move { size: 17 }
            }
            Button {
                class: "tool-button",
                variant: ButtonVariant::Ghost,
                size: ButtonSize::IconSm,
                aria_label: "Delete image",
                onclick: {
                    let ports = Rc::clone(&ports);
                    let image_id = props.image.id.clone();
                    move |_| {
                        let ports = Rc::clone(&ports);
                        let image_id = image_id.clone();
                        spawn(async move {
                            actions::run_mutation(
                                store,
                                ports,
                                crate::protocol::EditorRequest::DeleteImage {
                                    request_id: request_id("delete-image"),
                                    document_id: props.document_id,
                                    base_revision: props.revision,
                                    sheet_index: props.sheet_index,
                                    image_id,
                                },
                            )
                            .await;
                        });
                    }
                },
                Trash2 { size: 17 }
            }
            Label { html_for: "selected-image-width", title: "Image width",
                span { "W" }
                Input {
                    id: "selected-image-width",
                    r#type: "number",
                    min: 24,
                    max: 2000,
                    value: width_px,
                    aria_label: "Image width",
                    onchange: {
                        let ports = Rc::clone(&ports);
                        let image_id = props.image.id.clone();
                        let from = from.clone();
                        move |event: Event<FormData>| {
                            let Ok(width) = event.value().parse::<u32>() else { return; };
                            let ports = Rc::clone(&ports);
                            let image_id = image_id.clone();
                            let from = from.clone();
                            spawn(async move {
                                actions::run_mutation(
                                    store,
                                    ports,
                                    crate::protocol::EditorRequest::UpdateImage {
                                        request_id: request_id("resize-image"),
                                        document_id: props.document_id,
                                        base_revision: props.revision,
                                        sheet_index: props.sheet_index,
                                        image_id,
                                        anchor: ImageAnchorDto::OneCell {
                                            from,
                                            width_emu: width.clamp(24, 2000).saturating_mul(EMU_PER_PIXEL),
                                            height_emu,
                                        },
                                    },
                                )
                                .await;
                            });
                        }
                    }
                }
            }
            Label { html_for: "selected-image-height", title: "Image height",
                span { "H" }
                Input {
                    id: "selected-image-height",
                    r#type: "number",
                    min: 24,
                    max: 2000,
                    value: height_px,
                    aria_label: "Image height",
                    onchange: {
                        let ports = Rc::clone(&ports);
                        let image_id = props.image.id.clone();
                        move |event: Event<FormData>| {
                            let Ok(height) = event.value().parse::<u32>() else { return; };
                            let ports = Rc::clone(&ports);
                            let image_id = image_id.clone();
                            let from = from.clone();
                            spawn(async move {
                                actions::run_mutation(
                                    store,
                                    ports,
                                    crate::protocol::EditorRequest::UpdateImage {
                                        request_id: request_id("resize-image"),
                                        document_id: props.document_id,
                                        base_revision: props.revision,
                                        sheet_index: props.sheet_index,
                                        image_id,
                                        anchor: ImageAnchorDto::OneCell {
                                            from,
                                            width_emu,
                                            height_emu: height.clamp(24, 2000).saturating_mul(EMU_PER_PIXEL),
                                        },
                                    },
                                )
                                .await;
                            });
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn UpdateButton() -> Element {
    let mut update = use_signal(|| None);
    use_effect(move || {
        spawn(async move {
            if let Ok(available) = GitHubUpdatePort.check().await {
                update.set(available);
            }
        });
    });
    rsx! {
        if let Some(available) = update() {
            Button {
                class: "update-button",
                variant: ButtonVariant::Outline,
                size: ButtonSize::Xs,
                aria_label: "Open release page",
                onclick: move |_| PlatformWindowPort.open_external(&available.url),
                ExternalLink { size: 15 }
                "Update {available.version}"
            }
        }
    }
}

fn install_dirty_guard(dirty: bool) {
    #[cfg(any(feature = "web", feature = "desktop", feature = "mobile"))]
    {
        let guard = document::eval(
            "const dirty = await dioxus.recv(); window.onbeforeunload = dirty ? (event) => { event.preventDefault(); event.returnValue = ''; } : null;",
        );
        let _ = guard.send(dirty);
    }

    #[cfg(feature = "server")]
    let _ = dirty;
}

fn has_unsaved_changes(store: EditorStore) -> bool {
    store
        .document
        .read()
        .as_ref()
        .is_some_and(|document| document.editor_session.editor_state.is_dirty)
        || !store.pending_edits.read().is_empty()
}

fn request_editor_action(
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

async fn run_editor_action(
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
