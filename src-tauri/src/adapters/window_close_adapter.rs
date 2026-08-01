use tauri::{Emitter, Manager, Runtime, Window, WindowEvent};

pub(crate) const APPLICATION_CLOSE_REQUESTED_EVENT: &str = "application-close-requested";
const MAIN_WINDOW_LABEL: &str = "main";

pub(crate) fn intercept_main_window_close<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        intercept_main_window_close_actions(
            window.label(),
            || api.prevent_close(),
            || {
                if let Err(error) = window
                    .app_handle()
                    .emit(APPLICATION_CLOSE_REQUESTED_EVENT, ())
                {
                    eprintln!("Failed to emit application close request: {error}");
                }
            },
        );
    }
}

fn intercept_main_window_close_actions(
    window_label: &str,
    prevent_close: impl FnOnce(),
    notify_frontend: impl FnOnce(),
) {
    if window_label != MAIN_WINDOW_LABEL {
        return;
    }
    prevent_close();
    notify_frontend();
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::intercept_main_window_close_actions;

    #[test]
    fn main_window_is_prevented_before_the_frontend_is_notified() {
        let actions = RefCell::new(Vec::new());

        intercept_main_window_close_actions(
            "main",
            || actions.borrow_mut().push("prevent"),
            || actions.borrow_mut().push("notify"),
        );

        assert_eq!(*actions.borrow(), ["prevent", "notify"]);
    }

    #[test]
    fn auxiliary_window_close_is_not_intercepted() {
        let actions = RefCell::new(Vec::new());

        intercept_main_window_close_actions(
            "preview",
            || actions.borrow_mut().push("prevent"),
            || actions.borrow_mut().push("notify"),
        );

        assert!(actions.borrow().is_empty());
    }
}
