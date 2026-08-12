use std::rc::Rc;

use crate::ui::icons::{FilePlus, FolderOpen, Grid2X2Plus, HardDriveDownload, Trash2};
use dioxus::prelude::*;

use crate::Route;
use crate::actions;
use crate::model::{AppPorts, EditorStore};
use crate::ports::window::{PlatformWindowPort, WindowPort};

#[component]
pub fn HomeView() -> Element {
    let store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let navigator = use_navigator();

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
    rsx! {
        main { class: "home-shell",
            header { class: "home-header",
                div { class: "brand-lockup",
                    div { class: "brand-mark", Grid2X2Plus { size: 22 } }
                    div {
                        h1 { "Simple Table" }
                        p { "Spreadsheet editor" }
                    }
                }
                span { class: "platform-badge", "Rust / Dioxus" }
            }

            section { class: "home-actions", aria_label: "Create or open a spreadsheet",
                button {
                    class: "primary-command",
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

            if cfg!(any(feature = "web", feature = "server")) {
                section { class: "recent-section",
                    div { class: "section-heading",
                        div {
                            h2 { "Local workbooks" }
                            p { "Files saved in this browser stay on this device." }
                        }
                        HardDriveDownload { size: 19 }
                    }

                    if local_documents.is_empty() {
                        div { class: "empty-list",
                            FolderOpen { size: 30 }
                            p { "No local workbooks yet" }
                        }
                    } else {
                        div { class: "document-list",
                            for document in local_documents {
                                div { class: "document-row", key: "{document.id}",
                                    button {
                                        class: "document-open",
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
                                        span { class: "file-icon", FolderOpen { size: 18 } }
                                        span { class: "document-name", "{document.name}" }
                                        if document.has_recovery {
                                            span { class: "recovery-label", "Recovered" }
                                        }
                                    }
                                    button {
                                        class: "icon-button subtle",
                                        title: "Remove local workbook",
                                        aria_label: "Remove {document.name}",
                                        disabled: busy,
                                        onclick: {
                                            let ports = Rc::clone(&ports);
                                            let document_key = document.id;
                                            let document_name = document.name;
                                            move |_| {
                                                let ports = Rc::clone(&ports);
                                                let document_key = document_key.clone();
                                                let document_name = document_name.clone();
                                                spawn(async move {
                                                    if PlatformWindowPort
                                                        .confirm(
                                                            "Remove local workbook",
                                                            &format!("Permanently remove {document_name}?"),
                                                        )
                                                        .await
                                                    {
                                                        actions::delete_local_document(store, ports, document_key).await;
                                                    }
                                                });
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
            footer { class: "home-footer", "v{version}" }
        }
    }
}

#[component]
fn OpenDocumentControl() -> Element {
    let store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let navigator = use_navigator();
    let busy = store.busy();

    rsx! {
        label { class: if busy { "secondary-command disabled" } else { "secondary-command" },
            FolderOpen { size: 19 }
            span { "Open file" }
            input {
                class: "visually-hidden",
                r#type: "file",
                accept: ".xlsx,.csv,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet,text/csv",
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
