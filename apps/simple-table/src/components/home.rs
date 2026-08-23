use std::rc::Rc;

use dioxus::prelude::*;
#[cfg(not(feature = "mobile"))]
use simple_table_components::Label;
use simple_table_components::icons::{
    FilePlus, FolderOpen, Grid2x2Plus, HardDriveDownload, Trash2,
};
use simple_table_components::{
    AlertDialog, AlertDialogAction, AlertDialogActions, AlertDialogCancel, AlertDialogDescription,
    AlertDialogTitle, Badge, BadgeVariant, Button, ButtonSize, ButtonVariant, Item, ItemActions,
    ItemContent, ItemGroup, ItemMedia, ItemMediaVariant, ItemSize, ItemTitle,
};

use crate::Route;
use crate::actions;
use crate::model::{AppPorts, EditorStore};

#[component]
pub fn HomeView() -> Element {
    let store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let navigator = use_navigator();
    let mut pending_delete = use_signal(|| None::<(String, String)>);

    use_effect({
        let ports = Rc::clone(&ports);
        move || {
            let ports = Rc::clone(&ports);
            spawn(async move { actions::load_local_documents(store, ports).await });
        }
    });

    let busy = store.busy();
    let version = env!("CARGO_PKG_VERSION");
    let local_documents = store.local_documents.read().clone();
    let delete_target = pending_delete.read().clone();
    rsx! {
        main { class: "home-shell",
            header { class: "home-header",
                div { class: "brand-lockup",
                    div { class: "brand-mark", Grid2x2Plus { size: 22 } }
                    div {
                        h1 { "Simple Table" }
                        p { "Spreadsheet editor" }
                    }
                }
                Badge { class: "platform-badge", variant: BadgeVariant::Outline, "Rust / Dioxus" }
            }

            section { class: "home-actions", aria_label: "Create or open a spreadsheet",
                Button {
                    class: "primary-command",
                    size: ButtonSize::Lg,
                    disabled: busy,
                    onclick: {
                        let ports = Rc::clone(&ports);
                        move |_| {
                            let ports = Rc::clone(&ports);
                            spawn(async move {
                                if actions::new_document(store, ports).await {
                                    navigator.replace(Route::Table {});
                                }
                            });
                        }
                    },
                    FilePlus { size: 19 }
                    span { "New workbook" }
                }
                OpenDocumentControl {}
            }

            if cfg!(any(
                feature = "web",
                feature = "server",
                feature = "mobile"
            )) {
                section { class: "recent-section",
                    div { class: "section-heading",
                        div {
                            h2 { "Local workbooks" }
                            p { "Workbooks available on this device." }
                        }
                        HardDriveDownload { size: 19 }
                    }

                    if local_documents.is_empty() {
                        div { class: "empty-list",
                            FolderOpen { size: 30 }
                            p { "No local workbooks yet" }
                        }
                    } else {
                        ItemGroup { class: "document-list",
                            for document in local_documents {
                                Item { class: "document-row", key: "{document.id}", size: ItemSize::Sm,
                                    ItemMedia { variant: ItemMediaVariant::Icon,
                                        FolderOpen { size: 18 }
                                    }
                                    ItemContent {
                                    Button {
                                        class: "document-open",
                                        variant: ButtonVariant::Ghost,
                                        disabled: busy,
                                        onclick: {
                                            let ports = Rc::clone(&ports);
                                            let document_key = document.id.clone();
                                            move |_| {
                                                let ports = Rc::clone(&ports);
                                                let document_key = document_key.clone();
                                                spawn(async move {
                                                    if actions::open_local(store, ports, document_key).await {
                                                        navigator.replace(Route::Table {});
                                                    }
                                                });
                                            }
                                        },
                                        ItemTitle { class: "document-name", "{document.name}" }
                                        if document.has_recovery {
                                            Badge { class: "recovery-label", variant: BadgeVariant::Secondary, "Recovered" }
                                        }
                                    }
                                    }
                                    ItemActions {
                                    Button {
                                        class: "icon-button subtle",
                                        variant: ButtonVariant::Ghost,
                                        size: ButtonSize::IconSm,
                                        aria_label: "Remove {document.name}",
                                        disabled: busy,
                                        onclick: {
                                            let document_key = document.id;
                                            let document_name = document.name;
                                            move |_| {
                                                pending_delete.set(Some((
                                                    document_key.clone(),
                                                    document_name.clone(),
                                                )));
                                            }
                                        },
                                        Trash2 { size: 17 }
                                    }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            footer { class: "home-footer", "v{version}" }
            AlertDialog {
                open: Some(pending_delete.read().is_some()),
                on_open_change: move |open: bool| {
                    if !open {
                        pending_delete.set(None);
                    }
                },
                AlertDialogTitle { "Remove local workbook" }
                AlertDialogDescription {
                    if let Some((_, name)) = pending_delete.read().as_ref() {
                        "Permanently remove {name}? This cannot be undone."
                    }
                }
                AlertDialogActions {
                    AlertDialogCancel {
                        on_click: move |_| pending_delete.set(None),
                        "Cancel"
                    }
                    AlertDialogAction {
                        on_click: {
                            let ports = Rc::clone(&ports);
                            move |_| {
                                let Some((document_key, _)) = delete_target.clone() else {
                                    return;
                                };
                                let ports = Rc::clone(&ports);
                                spawn(async move {
                                    actions::delete_local_document(store, ports, document_key).await;
                                });
                            }
                        },
                        "Remove"
                    }
                }
            }
        }
    }
}

#[component]
fn OpenDocumentControl() -> Element {
    let store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let navigator = use_navigator();
    let busy = store.busy();

    #[cfg(feature = "mobile")]
    return rsx! {
        Button {
            class: "secondary-command",
            variant: ButtonVariant::Outline,
            size: ButtonSize::Lg,
            disabled: busy,
            onclick: move |_| {
                let ports = Rc::clone(&ports);
                spawn(async move {
                    match ports
                        .files
                        .pick_file(crate::ports::file::MobileFileKind::Workbook)
                        .await
                    {
                        Ok(Some(file)) => {
                            if actions::open_bytes(store, ports, file.name, file.bytes).await {
                                navigator.replace(Route::Table {});
                            }
                        }
                        Ok(None) => {}
                        Err(error) => store.set_error(error),
                    }
                });
            },
            FolderOpen { size: 19 }
            span { "Open file" }
        }
    };

    #[cfg(not(feature = "mobile"))]
    rsx! {
        Label {
            class: if busy { "secondary-command disabled" } else { "secondary-command" },
            html_for: "home-open-workbook",
            FolderOpen { size: 19 }
            span { "Open file" }
            input {
                id: "home-open-workbook",
                class: "visually-hidden",
                r#type: "file",
                accept: ".xlsx,.xlsm,.csv,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet,application/vnd.ms-excel.sheet.macroEnabled.12,text/csv",
                disabled: busy,
                onchange: {
                    let ports = Rc::clone(&ports);
                    move |event: Event<FormData>| {
                        let Some(file) = event.files().into_iter().next() else {
                            return;
                        };
                        let ports = Rc::clone(&ports);
                        spawn(async move {
                            let name = file.name();
                            match file.read_bytes().await {
                                Ok(bytes) => {
                                    if actions::open_bytes(store, ports, name, bytes.to_vec()).await {
                                        navigator.replace(Route::Table {});
                                    }
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
}
