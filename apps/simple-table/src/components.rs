mod editor;
mod grid;
mod home;
mod search;

pub use editor::EditorView;
pub use home::HomeView;

use dioxus::prelude::*;
use simple_table_components::{ToastOptions, use_toast};
use std::time::Duration;

use crate::model::EditorStore;

const TRANSIENT_NOTICE_DURATION: Duration = Duration::from_secs(8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ErrorToastPresentation {
    title: &'static str,
    permanent: bool,
}

fn error_toast_presentation(code: &str) -> ErrorToastPresentation {
    let (title, permanent) = match code {
        "write_error" | "file_target_unavailable" => ("Could not save file", true),
        "read_error" => ("Could not read file", false),
        "file_not_found" | "not_found" | "mobile_recovery_missing" => ("File unavailable", false),
        "unsupported_format" | "unsupported_attachment" => ("Unsupported file", false),
        "resource_limit_exceeded" => ("Resource limit reached", false),
        "browser_error" | "mobile_file_error" | "android_file_error" => {
            ("File operation failed", false)
        }
        "indexed_db_error"
        | "indexed_db_serialization_error"
        | "memory_store_error"
        | "mobile_recovery_error"
        | "storage_error" => ("Local storage unavailable", true),
        "worker_failed"
        | "worker_disconnected"
        | "worker_start_failed"
        | "worker_protocol_error"
        | "editor_task_failed"
        | "workspace_unavailable" => ("Editor unavailable", true),
        "transaction_rollback_failed" | "document_state_invalid" => {
            ("Workbook state unavailable", true)
        }
        "workbook_patch_failed"
        | "document_changed"
        | "document_closed"
        | "stale_mutation_response" => ("Could not apply changes", false),
        "region_loader_stopped"
        | "region_response_too_large"
        | "region_split_limit"
        | "stale_region_response" => ("Could not load sheet", false),
        "nothing_to_undo"
        | "nothing_to_redo"
        | "cannot_delete_last_sheet"
        | "invalid_sheet_index"
        | "invalid_cell_position"
        | "row_not_found"
        | "no_document"
        | "no_file_loaded"
        | "prepared_document_conflict"
        | "unsupported_workbook_structure" => ("Action unavailable", false),
        "update_error" => ("Update check failed", false),
        "internal" | "invalid_request" | "protocol_error" | "unexpected_reply" => {
            ("Unexpected error", true)
        }
        _ => ("Action failed", false),
    };
    ErrorToastPresentation { title, permanent }
}

#[component]
pub fn ToastBridge() -> Element {
    let mut store = use_context::<EditorStore>();
    let toasts = use_toast();
    use_effect(move || {
        let error = store.error.read().clone();
        if let Some(error) = error {
            store.error.set(None);
            let presentation = error_toast_presentation(&error.code);
            let options = ToastOptions::new().description(error.message);
            let options = if presentation.permanent {
                options.permanent(true)
            } else {
                options.duration(TRANSIENT_NOTICE_DURATION)
            };
            toasts.error(presentation.title.to_string(), options);
        }
    });
    use_effect(move || {
        let warning = store.warning.read().clone();
        if let Some(warning) = warning {
            store.warning.set(None);
            let options = ToastOptions::new().description(warning.message);
            let options = if warning.permanent {
                options.permanent(true)
            } else {
                options.duration(TRANSIENT_NOTICE_DURATION)
            };
            toasts.warning(warning.title, options);
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
                ToastBridge {}
            }
        }
    }

    #[component]
    fn ToastBurst() -> Element {
        let toasts = use_toast();
        use_hook(move || {
            for index in 0..4 {
                toasts.info(
                    format!("Notice {index}"),
                    ToastOptions::new().permanent(true),
                );
            }
        });
        rsx! {}
    }

    #[component]
    fn ToastLimitHarness() -> Element {
        rsx! {
            ToastProvider { max_toasts: 3usize,
                ToastBurst {}
            }
        }
    }

    #[test]
    fn error_codes_map_to_user_facing_presentations() {
        for (code, title, permanent) in [
            ("write_error", "Could not save file", true),
            ("read_error", "Could not read file", false),
            ("unsupported_format", "Unsupported file", false),
            ("resource_limit_exceeded", "Resource limit reached", false),
            ("indexed_db_error", "Local storage unavailable", true),
            ("worker_disconnected", "Editor unavailable", true),
            ("document_state_invalid", "Workbook state unavailable", true),
            ("document_changed", "Could not apply changes", false),
            ("region_split_limit", "Could not load sheet", false),
            ("nothing_to_undo", "Action unavailable", false),
            ("update_error", "Update check failed", false),
            ("protocol_error", "Unexpected error", true),
            ("future_error", "Action failed", false),
        ] {
            assert_eq!(
                error_toast_presentation(code),
                ErrorToastPresentation { title, permanent },
                "unexpected presentation for {code}",
            );
        }
    }

    #[test]
    fn error_created_after_mount_is_forwarded_to_a_toast() {
        let mut dom = VirtualDom::new(ToastHarness);
        dom.rebuild_in_place();
        dom.render_immediate_to_vec();
        let store = TEST_STORE.with(|slot| slot.borrow().expect("captured editor store"));

        store.set_error(AppErrorDto {
            code: "write_error".to_string(),
            message: "The selected file could not be written".to_string(),
        });
        dom.render_immediate_to_vec();
        dom.render_immediate_to_vec();
        let html = dioxus_ssr::render(&dom);

        assert!(html.contains("Could not save file"));
        assert!(html.contains("The selected file could not be written"));
        assert!(!html.contains("write_error"));
        assert!(html.contains("data-permanent=true"));
        assert!(store.error.read().is_none());
        TEST_STORE.with(|slot| slot.replace(None));
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn warning_created_after_mount_is_forwarded_to_a_non_permanent_toast() {
        let runtime = tokio::runtime::Runtime::new().expect("test runtime");
        let _runtime_guard = runtime.enter();
        let mut dom = VirtualDom::new(ToastHarness);
        dom.rebuild_in_place();
        dom.render_immediate_to_vec();
        let store = TEST_STORE.with(|slot| slot.borrow().expect("captured editor store"));

        store.report_recovery_failure(crate::model::UiNotice {
            title: "Recovery cleanup failed".to_string(),
            message: "An older recovery copy could not be removed".to_string(),
            permanent: false,
        });
        dom.render_immediate_to_vec();
        dom.render_immediate_to_vec();
        let html = dioxus_ssr::render(&dom);

        assert!(html.contains("Recovery cleanup failed"));
        assert!(html.contains("An older recovery copy could not be removed"));
        assert!(!html.contains("data-permanent=true"));
        assert!(store.warning.read().is_none());
        TEST_STORE.with(|slot| slot.replace(None));
    }

    #[test]
    fn recovery_checkpoint_warning_is_permanent() {
        let mut dom = VirtualDom::new(ToastHarness);
        dom.rebuild_in_place();
        dom.render_immediate_to_vec();
        let store = TEST_STORE.with(|slot| slot.borrow().expect("captured editor store"));

        store.report_recovery_failure(crate::model::UiNotice {
            title: "Automatic recovery unavailable".to_string(),
            message: "Save the workbook manually".to_string(),
            permanent: true,
        });
        dom.render_immediate_to_vec();
        dom.render_immediate_to_vec();
        let html = dioxus_ssr::render(&dom);

        assert!(html.contains("Automatic recovery unavailable"));
        assert!(html.contains("Save the workbook manually"));
        assert!(html.contains("data-permanent=true"));
        assert!(store.warning.read().is_none());
        TEST_STORE.with(|slot| slot.replace(None));
    }

    #[test]
    fn toast_provider_keeps_at_most_three_notifications() {
        let mut dom = VirtualDom::new(ToastLimitHarness);
        dom.rebuild_in_place();
        dom.mark_all_dirty();
        dom.render_immediate_to_vec();
        dom.render_immediate_to_vec();
        let html = dioxus_ssr::render(&dom);

        assert_eq!(html.matches("data-type=\"info\"").count(), 3, "{html}");
        assert!(!html.contains("Notice 0"));
        assert!(html.contains("Notice 1"));
        assert!(html.contains("Notice 2"));
        assert!(html.contains("Notice 3"));
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
