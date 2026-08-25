use std::rc::Rc;

pub trait WindowPort {
    fn open_external(&self, url: &str);

    fn set_unsaved_changes_warning(&self, enabled: bool);
}

pub struct PlatformWindowPort;

pub fn platform_window_port() -> Rc<dyn WindowPort> {
    Rc::new(PlatformWindowPort)
}

impl WindowPort for PlatformWindowPort {
    fn open_external(&self, url: &str) {
        #[cfg(target_arch = "wasm32")]
        if let Some(window) = web_sys::window() {
            let _ = window.open_with_url_and_target(url, "_blank");
        }

        #[cfg(all(not(target_arch = "wasm32"), feature = "desktop"))]
        {
            let _ = webbrowser::open(url);
        }

        #[cfg(feature = "mobile")]
        {
            let eval = dioxus::document::eval("window.open(await dioxus.recv(), '_blank');");
            let _ = eval.send(url);
        }

        #[cfg(feature = "server")]
        let _ = url;
    }

    fn set_unsaved_changes_warning(&self, enabled: bool) {
        #[cfg(any(feature = "web", feature = "desktop", feature = "mobile"))]
        {
            let guard = dioxus::document::eval(
                "const dirty = await dioxus.recv(); window.onbeforeunload = dirty ? (event) => { event.preventDefault(); event.returnValue = ''; } : null;",
            );
            let _ = guard.send(enabled);
        }

        #[cfg(feature = "server")]
        let _ = enabled;
    }
}
