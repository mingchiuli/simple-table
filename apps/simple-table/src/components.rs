mod editor;
mod grid;
mod home;
mod search;

pub use editor::EditorView;
pub use home::HomeView;

use dioxus::prelude::*;
use simple_table_components::{ToastOptions, use_toast};

use crate::model::EditorStore;

#[component]
pub fn ErrorToastBridge() -> Element {
    let mut store = use_context::<EditorStore>();
    let error = store.error.read().clone();
    let toasts = use_toast();
    use_effect(move || {
        if let Some(error) = error.clone() {
            store.error.set(None);
            toasts.error(
                error.code,
                ToastOptions::new()
                    .description(error.message)
                    .permanent(true),
            );
        }
    });
    rsx! {}
}
