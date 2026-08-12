use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use crate::protocol::AppErrorDto;

pub type FileFuture<T> = Pin<Box<dyn Future<Output = T> + 'static>>;

pub trait FilePort {
    fn write_document(
        &self,
        suggested_name: String,
        bytes: Vec<u8>,
    ) -> FileFuture<Result<Option<String>, AppErrorDto>>;
}

#[cfg(feature = "desktop")]
pub fn platform_file_port() -> Rc<dyn FilePort> {
    Rc::new(native::NativeFilePort)
}

#[cfg(all(not(feature = "desktop"), feature = "mobile"))]
pub fn platform_file_port() -> Rc<dyn FilePort> {
    Rc::new(mobile::MobileFilePort)
}

#[cfg(all(not(feature = "desktop"), not(feature = "mobile"), feature = "web"))]
pub fn platform_file_port() -> Rc<dyn FilePort> {
    Rc::new(web::WebFilePort)
}

#[cfg(not(any(feature = "desktop", feature = "mobile", feature = "web")))]
pub fn platform_file_port() -> Rc<dyn FilePort> {
    Rc::new(UnavailableFilePort)
}

#[cfg(not(any(feature = "desktop", feature = "mobile", feature = "web")))]
struct UnavailableFilePort;

#[cfg(not(any(feature = "desktop", feature = "mobile", feature = "web")))]
impl FilePort for UnavailableFilePort {
    fn write_document(
        &self,
        _suggested_name: String,
        _bytes: Vec<u8>,
    ) -> FileFuture<Result<Option<String>, AppErrorDto>> {
        Box::pin(async { Ok(None) })
    }
}

#[cfg(feature = "desktop")]
mod native {
    use super::*;

    pub struct NativeFilePort;

    impl FilePort for NativeFilePort {
        fn write_document(
            &self,
            suggested_name: String,
            bytes: Vec<u8>,
        ) -> FileFuture<Result<Option<String>, AppErrorDto>> {
            Box::pin(async move {
                let Some(file) = rfd::AsyncFileDialog::new()
                    .set_file_name(&suggested_name)
                    .add_filter("Excel workbook", &["xlsx"])
                    .add_filter("CSV", &["csv"])
                    .save_file()
                    .await
                else {
                    return Ok(None);
                };
                let path = file.path().to_path_buf();
                tokio::task::spawn_blocking({
                    let path = path.clone();
                    move || simple_table_engine::write_native_file_atomically(&path, &bytes)
                })
                .await
                .map_err(|error| AppErrorDto {
                    code: "write_error".to_string(),
                    message: error.to_string(),
                })??;
                Ok(Some(path.to_string_lossy().into_owned()))
            })
        }
    }
}

#[cfg(feature = "mobile")]
mod mobile {
    use base64::Engine;
    use dioxus::document;

    use super::*;

    pub struct MobileFilePort;

    impl FilePort for MobileFilePort {
        fn write_document(
            &self,
            suggested_name: String,
            bytes: Vec<u8>,
        ) -> FileFuture<Result<Option<String>, AppErrorDto>> {
            Box::pin(async move {
                let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                let eval = document::eval(
                    "const payload = await dioxus.recv();\n\
                     const anchor = document.createElement('a');\n\
                     anchor.href = `data:application/octet-stream;base64,${payload.bytes}`;\n\
                     anchor.download = payload.name;\n\
                     document.body.appendChild(anchor);\n\
                     anchor.click();\n\
                     anchor.remove();",
                );
                eval.send(serde_json::json!({
                    "name": suggested_name,
                    "bytes": encoded,
                }))
                .map_err(eval_error)?;
                Ok(Some(suggested_name))
            })
        }
    }

    fn eval_error(error: dioxus::document::EvalError) -> AppErrorDto {
        AppErrorDto {
            code: "mobile_file_error".to_string(),
            message: error.to_string(),
        }
    }
}

#[cfg(feature = "web")]
mod web {
    use wasm_bindgen::JsCast;

    use super::*;

    pub struct WebFilePort;

    impl FilePort for WebFilePort {
        fn write_document(
            &self,
            suggested_name: String,
            bytes: Vec<u8>,
        ) -> FileFuture<Result<Option<String>, AppErrorDto>> {
            Box::pin(async move {
                let parts = js_sys::Array::new();
                parts.push(&js_sys::Uint8Array::from(bytes.as_slice()));
                let blob =
                    web_sys::Blob::new_with_u8_array_sequence(&parts).map_err(browser_error)?;
                let url =
                    web_sys::Url::create_object_url_with_blob(&blob).map_err(browser_error)?;
                let document = web_sys::window()
                    .and_then(|window| window.document())
                    .ok_or_else(|| AppErrorDto {
                        code: "browser_error".to_string(),
                        message: "browser document is unavailable".to_string(),
                    })?;
                let anchor = document
                    .create_element("a")
                    .map_err(browser_error)?
                    .dyn_into::<web_sys::HtmlAnchorElement>()
                    .map_err(|_| AppErrorDto {
                        code: "browser_error".to_string(),
                        message: "failed to create a download link".to_string(),
                    })?;
                anchor.set_href(&url);
                anchor.set_download(&suggested_name);
                anchor.click();
                web_sys::Url::revoke_object_url(&url).map_err(browser_error)?;
                Ok(Some(suggested_name))
            })
        }
    }

    fn browser_error(value: wasm_bindgen::JsValue) -> AppErrorDto {
        AppErrorDto {
            code: "browser_error".to_string(),
            message: value.as_string().unwrap_or_else(|| format!("{value:?}")),
        }
    }
}
