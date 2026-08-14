mod editor;
mod grid;
mod home;
mod search;

pub use editor::EditorView;
pub use home::HomeView;

use crate::ui::icons::X;
use dioxus::prelude::*;

use crate::model::EditorStore;

#[component]
pub fn ErrorNotice() -> Element {
    let mut store = use_context::<EditorStore>();
    let error = store.error.read().clone();
    rsx! {
        if let Some(error) = error {
            div { class: "error-notice", role: "alert",
                div {
                    strong { "{error.code}" }
                    span { "{error.message}" }
                }
                button {
                    class: "icon-button",
                    title: "Dismiss",
                    aria_label: "Dismiss error",
                    onclick: move |_| store.error.set(None),
                    X { size: 17 }
                }
            }
        }
    }
}
