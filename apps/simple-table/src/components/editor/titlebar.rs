use std::rc::Rc;

use dioxus::prelude::*;
use dioxus::router::use_navigator;
use simple_table_components::icons::{ArrowLeft, ExternalLink, Sheet};
use simple_table_components::{Button, ButtonSize, ButtonVariant};

use super::{EditorUiState, PendingEditorAction, has_unsaved_changes, request_editor_action};
use crate::model::{AppPorts, EditorStore};
use crate::ports::window::WindowPort;

#[component]
pub(super) fn EditorTitlebar() -> Element {
    let store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let navigator = use_navigator();
    let ui_state = use_context::<EditorUiState>();
    let document = store.document.read();
    let Some(document) = document.as_ref() else {
        return rsx! {};
    };
    let file_name = document.document.file_name.clone();
    let dirty = has_unsaved_changes(store);

    rsx! {
        header { class: "editor-titlebar",
            Button {
                class: "icon-button editor-back-button",
                variant: ButtonVariant::Ghost,
                size: ButtonSize::IconSm,
                aria_label: "Back to files",
                disabled: store.busy(),
                onclick: {
                    let ports = Rc::clone(&ports);
                    move |_| {
                        request_editor_action(
                            PendingEditorAction::Close,
                            ui_state.pending_action,
                            store,
                            Rc::clone(&ports),
                            navigator,
                        );
                    }
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
    }
}

#[component]
fn UpdateButton() -> Element {
    let ports = use_context::<Rc<AppPorts>>();
    let mut update = use_signal(|| None);
    let update_port = Rc::clone(&ports.update);
    use_effect(move || {
        let update_port = Rc::clone(&update_port);
        spawn(async move {
            if let Ok(available) = update_port.check().await {
                update.set(available);
            }
        });
    });
    let window = Rc::clone(&ports.window);

    rsx! {
        if let Some(available) = update() {
            Button {
                class: "update-button",
                variant: ButtonVariant::Outline,
                size: ButtonSize::Xs,
                aria_label: "Open release page",
                onclick: move |_| open_update(window.as_ref(), &available.url),
                ExternalLink { size: 15 }
                "Update {available.version}"
            }
        }
    }
}

fn open_update(window: &dyn WindowPort, url: &str) {
    window.open_external(url);
}

#[cfg(all(test, not(feature = "mobile")))]
mod tests {
    use std::cell::RefCell;
    use std::future::Future;
    use std::pin::Pin;

    use super::*;
    use crate::ports::update::{AvailableUpdate, UpdatePort};

    thread_local! {
        static TEST_PORTS: RefCell<Option<Rc<AppPorts>>> = const { RefCell::new(None) };
    }

    #[derive(Default)]
    struct RecordingWindowPort {
        opened: Rc<RefCell<Vec<String>>>,
    }

    impl WindowPort for RecordingWindowPort {
        fn open_external(&self, url: &str) {
            self.opened.borrow_mut().push(url.to_string());
        }

        fn set_unsaved_changes_warning(&self, _enabled: bool) {}
    }

    struct FixedUpdatePort(Result<Option<AvailableUpdate>, crate::protocol::AppErrorDto>);

    impl UpdatePort for FixedUpdatePort {
        fn check(
            &self,
        ) -> Pin<
            Box<dyn Future<Output = Result<Option<AvailableUpdate>, crate::protocol::AppErrorDto>>>,
        > {
            let result = self.0.clone();
            Box::pin(async move { result })
        }
    }

    #[component]
    fn UpdateHarness() -> Element {
        let ports = TEST_PORTS.with(|slot| {
            slot.borrow()
                .as_ref()
                .cloned()
                .expect("test ports installed")
        });
        use_context_provider(|| ports);
        rsx! { UpdateButton {} }
    }

    fn test_ports(update: Rc<dyn UpdatePort>, window: Rc<dyn WindowPort>) -> Rc<AppPorts> {
        let (editor, workspace) = crate::ports::platform_editor_and_workspace_ports();
        Rc::new(AppPorts {
            regions: crate::actions::RegionLoader::new(Rc::clone(&editor)),
            editor,
            files: crate::ports::file::platform_file_port(),
            update,
            window,
            workspace,
            operations: Rc::new(futures::lock::Mutex::new(())),
        })
    }

    fn render_update(
        result: Result<Option<AvailableUpdate>, crate::protocol::AppErrorDto>,
    ) -> String {
        let ports = test_ports(
            Rc::new(FixedUpdatePort(result)),
            Rc::new(RecordingWindowPort::default()),
        );
        TEST_PORTS.with(|slot| slot.replace(Some(ports)));

        let mut dom = VirtualDom::new(UpdateHarness);
        dom.rebuild_in_place();
        dom.render_immediate_to_vec();
        dom.render_immediate_to_vec();
        dom.render_immediate_to_vec();
        let html = dioxus_ssr::render(&dom);
        TEST_PORTS.with(|slot| slot.replace(None));
        html
    }

    #[test]
    fn injected_update_port_controls_rendered_update() {
        let update = AvailableUpdate {
            version: "9.9.9".to_string(),
            url: "https://example.com/release".to_string(),
        };
        let html = render_update(Ok(Some(update)));

        assert!(html.contains("Update 9.9.9"));
    }

    #[test]
    fn missing_or_failed_update_checks_do_not_render_a_button() {
        let missing = render_update(Ok(None));
        let failed = render_update(Err(crate::protocol::AppErrorDto {
            code: "update_error".to_string(),
            message: "offline".to_string(),
        }));

        assert!(!missing.contains("update-button"));
        assert!(!failed.contains("update-button"));
    }

    #[test]
    fn update_link_uses_the_injected_window_port() {
        let opened = Rc::new(RefCell::new(Vec::new()));
        let window = RecordingWindowPort {
            opened: Rc::clone(&opened),
        };

        open_update(&window, "https://example.com/release");

        assert_eq!(
            opened.borrow().as_slice(),
            &["https://example.com/release".to_string()]
        );
    }
}
