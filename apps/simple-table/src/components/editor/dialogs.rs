use std::rc::Rc;

use dioxus::prelude::*;
use dioxus::router::use_navigator;
#[cfg(feature = "mobile")]
use simple_table_components::icons::Save;
use simple_table_components::{
    AlertDialog, AlertDialogAction, AlertDialogActions, AlertDialogCancel, AlertDialogDescription,
    AlertDialogTitle,
};
#[cfg(feature = "mobile")]
use simple_table_components::{Button, ButtonVariant, Dialog, DialogTitle, Input, Label};

use super::{EditorUiState, run_editor_action};
#[cfg(feature = "mobile")]
use crate::actions;
use crate::model::{AppPorts, EditorStore};

#[component]
pub(super) fn UnsavedChangesDialog() -> Element {
    let store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let navigator = use_navigator();
    let ui_state = use_context::<EditorUiState>();
    let mut pending_action = ui_state.pending_action;
    let confirmed_action = pending_action.read().clone();

    rsx! {
        if pending_action.read().is_some() {
            AlertDialog {
                open: Some(true),
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

#[cfg(feature = "mobile")]
#[component]
pub(super) fn MobileSaveDialog() -> Element {
    let store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let ui_state = use_context::<EditorUiState>();
    let mut pending_name = ui_state.pending_save_name;
    let Some(name) = pending_name.read().clone() else {
        return rsx! {};
    };
    let target_name = name.trim().to_string();
    let can_save = !target_name.is_empty();

    rsx! {
        Dialog {
            open: Some(true),
            on_open_change: move |open: bool| {
                if !open {
                    pending_name.set(None);
                }
            },
            DialogTitle { "Save workbook" }
            form {
                class: "save-name-form",
                onsubmit: {
                    let ports = Rc::clone(&ports);
                    move |event: FormEvent| {
                        event.prevent_default();
                        if target_name.is_empty() {
                            return;
                        }
                        pending_name.set(None);
                        let ports = Rc::clone(&ports);
                        let target_name = target_name.clone();
                        spawn(async move {
                            actions::save_local_as(store, ports, target_name).await;
                        });
                    }
                },
                Label { html_for: "mobile-save-name", "File name" }
                Input {
                    id: "mobile-save-name",
                    value: name,
                    oninput: move |event: FormEvent| pending_name.set(Some(event.value()))
                }
                div { class: "save-name-actions",
                    Button {
                        r#type: "button",
                        variant: ButtonVariant::Outline,
                        onclick: move |_| pending_name.set(None),
                        "Cancel"
                    }
                    Button { r#type: "submit", disabled: !can_save,
                        Save { size: 16 }
                        "Save"
                    }
                }
            }
        }
    }
}
