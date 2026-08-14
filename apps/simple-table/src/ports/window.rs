use std::future::Future;
use std::pin::Pin;

pub type WindowFuture<T> = Pin<Box<dyn Future<Output = T> + 'static>>;

pub trait WindowPort {
    fn open_external(&self, url: &str);
    fn confirm(&self, title: &str, message: &str) -> WindowFuture<bool>;
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

    fn confirm(&self, title: &str, message: &str) -> WindowFuture<bool> {
        let title = title.to_string();
        let message = message.to_string();
        Box::pin(async move {
            #[cfg(target_arch = "wasm32")]
            {
                let _ = title;
                web_sys::window()
                    .and_then(|window| window.confirm_with_message(&message).ok())
                    .unwrap_or(false)
            }

            #[cfg(all(not(target_arch = "wasm32"), feature = "desktop"))]
            {
                rfd::AsyncMessageDialog::new()
                    .set_title(title)
                    .set_description(message)
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show()
                    .await
                    == rfd::MessageDialogResult::Yes
            }

            #[cfg(all(
                not(target_arch = "wasm32"),
                not(feature = "desktop"),
                feature = "mobile"
            ))]
            {
                let mut eval = dioxus::document::eval(
                    "const payload = await dioxus.recv(); dioxus.send(window.confirm(payload.message));",
                );
                if eval
                    .send(serde_json::json!({ "title": title, "message": message }))
                    .is_err()
                {
                    return false;
                }
                eval.recv::<bool>().await.unwrap_or(false)
            }

            #[cfg(all(
                not(target_arch = "wasm32"),
                not(feature = "desktop"),
                not(feature = "mobile")
            ))]
            {
                let _ = (title, message);
                false
            }
        })
    }
}
