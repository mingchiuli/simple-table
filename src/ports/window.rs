pub trait WindowPort {
    fn open_external(&self, url: &str);
}

pub struct PlatformWindowPort;

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
}
