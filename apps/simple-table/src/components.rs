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
    let toasts = use_toast();
    use_effect(move || {
        let error = store.error.read().clone();
        if let Some(error) = error {
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::model::use_editor_store;
    use crate::protocol::AppErrorDto;
    use simple_table_components::ToastProvider;

    thread_local! {
        #[allow(clippy::missing_const_for_thread_local)]
        static TEST_STORE: RefCell<Option<EditorStore>> = const { RefCell::new(None) };
    }

    #[component]
    fn ToastHarness() -> Element {
        let store = use_editor_store();
        TEST_STORE.with(|slot| slot.replace(Some(store)));
        use_context_provider(|| store);
        rsx! {
            ToastProvider {
                ErrorToastBridge {}
            }
        }
    }

    #[test]
    fn error_created_after_mount_is_forwarded_to_a_toast() {
        let mut dom = VirtualDom::new(ToastHarness);
        dom.rebuild_in_place();
        dom.render_immediate_to_vec();
        let store = TEST_STORE.with(|slot| slot.borrow().expect("captured editor store"));

        store.set_error(AppErrorDto {
            code: "late_error".to_string(),
            message: "Raised after the initial render".to_string(),
        });
        dom.render_immediate_to_vec();
        dom.render_immediate_to_vec();
        let html = dioxus_ssr::render(&dom);

        assert!(html.contains("late_error"));
        assert!(html.contains("Raised after the initial render"));
        assert!(store.error.read().is_none());
        TEST_STORE.with(|slot| slot.replace(None));
    }

    #[test]
    fn operation_guard_owns_busy_until_the_current_operation_finishes() {
        let mut dom = VirtualDom::new(ToastHarness);
        dom.rebuild_in_place();
        let store = TEST_STORE.with(|slot| slot.borrow().expect("captured editor store"));
        let first = store.begin_operation("First");
        let second = store.begin_operation("Second");

        drop(first);
        assert!(store.busy());
        store.set_error(AppErrorDto {
            code: "background_error".to_string(),
            message: "Background load failed".to_string(),
        });
        assert!(store.busy());
        drop(second);
        assert!(!store.busy());
        TEST_STORE.with(|slot| slot.replace(None));
    }
}
