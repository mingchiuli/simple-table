use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use crate::protocol::AppErrorDto;

pub type FileFuture<T> = Pin<Box<dyn Future<Output = T> + 'static>>;

#[cfg(feature = "mobile")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MobileFileKind {
    Workbook,
    Image,
}

#[cfg(feature = "mobile")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickedFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

#[cfg(feature = "desktop")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentDialogMode {
    Save,
    Export,
}

pub trait FilePort {
    #[cfg(feature = "mobile")]
    fn pick_file(
        &self,
        kind: MobileFileKind,
    ) -> FileFuture<Result<Option<PickedFile>, AppErrorDto>>;

    #[cfg(not(feature = "desktop"))]
    fn write_document(
        &self,
        suggested_name: String,
        bytes: Vec<u8>,
    ) -> FileFuture<Result<Option<String>, AppErrorDto>>;

    #[cfg(feature = "mobile")]
    fn write_document_to_target(
        &self,
        existing_target: Option<String>,
        suggested_name: String,
        bytes: Vec<u8>,
    ) -> FileFuture<Result<Option<String>, AppErrorDto>> {
        let _ = existing_target;
        self.write_document(suggested_name, bytes)
    }

    #[cfg(feature = "desktop")]
    fn choose_document_path(
        &self,
        _suggested_name: String,
        _mode: DocumentDialogMode,
    ) -> FileFuture<Result<Option<String>, AppErrorDto>> {
        Box::pin(async { Ok(None) })
    }

    #[cfg(feature = "desktop")]
    fn write_document_to_path(
        &self,
        _path: String,
        _bytes: Vec<u8>,
    ) -> FileFuture<Result<(), AppErrorDto>> {
        Box::pin(async {
            Err(AppErrorDto {
                code: "file_target_unavailable".to_string(),
                message: "writing to a selected path is unavailable on this platform".to_string(),
            })
        })
    }
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
        fn choose_document_path(
            &self,
            suggested_name: String,
            mode: DocumentDialogMode,
        ) -> FileFuture<Result<Option<String>, AppErrorDto>> {
            Box::pin(async move { Ok(choose_path(suggested_name, mode).await) })
        }

        fn write_document_to_path(
            &self,
            path: String,
            bytes: Vec<u8>,
        ) -> FileFuture<Result<(), AppErrorDto>> {
            Box::pin(write_path(path, bytes))
        }
    }

    async fn choose_path(suggested_name: String, mode: DocumentDialogMode) -> Option<String> {
        let dialog = rfd::AsyncFileDialog::new()
            .set_file_name(&suggested_name)
            .add_filter("Excel workbook", &["xlsx", "xlsm"]);
        let dialog = if mode == DocumentDialogMode::Export
            || suggested_name.to_ascii_lowercase().ends_with(".csv")
        {
            dialog.add_filter("CSV", &["csv"])
        } else {
            dialog
        };
        dialog
            .save_file()
            .await
            .map(|file| file.path().to_string_lossy().into_owned())
    }

    async fn write_path(path: String, bytes: Vec<u8>) -> Result<(), AppErrorDto> {
        tokio::task::spawn_blocking(move || {
            simple_table_engine::write_native_file_atomically(std::path::Path::new(&path), &bytes)
        })
        .await
        .map_err(|error| AppErrorDto {
            code: "write_error".to_string(),
            message: error.to_string(),
        })??;
        Ok(())
    }
}

#[cfg(feature = "mobile")]
mod mobile {
    use super::*;

    pub struct MobileFilePort;

    impl FilePort for MobileFilePort {
        fn pick_file(
            &self,
            kind: MobileFileKind,
        ) -> FileFuture<Result<Option<PickedFile>, AppErrorDto>> {
            Box::pin(pick_file(kind))
        }

        fn write_document(
            &self,
            suggested_name: String,
            bytes: Vec<u8>,
        ) -> FileFuture<Result<Option<String>, AppErrorDto>> {
            #[cfg(target_os = "android")]
            return Box::pin(crate::ports::android::write_document(
                None,
                suggested_name,
                bytes,
            ));

            #[cfg(target_os = "ios")]
            return Box::pin(write_ios_document(None, suggested_name, bytes, true));

            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            Box::pin(async move {
                let _ = (suggested_name, bytes);
                Ok(None)
            })
        }

        fn write_document_to_target(
            &self,
            existing_target: Option<String>,
            suggested_name: String,
            bytes: Vec<u8>,
        ) -> FileFuture<Result<Option<String>, AppErrorDto>> {
            #[cfg(target_os = "android")]
            return Box::pin(crate::ports::android::write_document(
                existing_target,
                suggested_name,
                bytes,
            ));

            #[cfg(target_os = "ios")]
            return Box::pin(write_ios_document(
                existing_target,
                suggested_name,
                bytes,
                false,
            ));

            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            Box::pin(async move {
                let _ = (existing_target, suggested_name, bytes);
                Ok(None)
            })
        }
    }

    #[derive(serde::Deserialize)]
    struct PickedFilePayload {
        name: String,
        data: String,
    }

    async fn pick_file(kind: MobileFileKind) -> Result<Option<PickedFile>, AppErrorDto> {
        use base64::Engine;
        use dioxus::document;

        // Dioxus 0.7.10's native file dialog returns no files on Android and iOS.
        // Let the platform WebView handle its standard HTML file picker instead.
        let accept = match kind {
            MobileFileKind::Workbook => {
                ".xlsx,.xlsm,.csv,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet,application/vnd.ms-excel.sheet.macroEnabled.12,text/csv"
            }
            MobileFileKind::Image => "image/png,image/jpeg",
        };
        let mut eval = document::eval(
            "const accept = await dioxus.recv();\n\
             const input = document.createElement('input');\n\
             input.type = 'file';\n\
             input.accept = accept;\n\
             input.style.display = 'none';\n\
             let settled = false;\n\
             const finish = (value) => {\n\
                 if (settled) return;\n\
                 settled = true;\n\
                 input.remove();\n\
                 dioxus.send(value);\n\
             };\n\
             input.addEventListener('change', () => {\n\
                 const file = input.files && input.files[0];\n\
                 if (!file) { finish(null); return; }\n\
                 const reader = new FileReader();\n\
                 reader.onload = () => finish({ name: file.name, data: reader.result });\n\
                 reader.onerror = () => finish({ name: file.name, data: '' });\n\
                 reader.readAsDataURL(file);\n\
             }, { once: true });\n\
             input.addEventListener('cancel', () => finish(null), { once: true });\n\
             window.addEventListener('focus', () => {\n\
                 setTimeout(() => {\n\
                     if (!settled && (!input.files || input.files.length === 0)) finish(null);\n\
                 }, 300);\n\
             }, { once: true });\n\
             document.body.appendChild(input);\n\
             input.click();",
        );
        eval.send(accept).map_err(eval_error)?;
        let Some(payload) = eval
            .recv::<Option<PickedFilePayload>>()
            .await
            .map_err(eval_error)?
        else {
            return Ok(None);
        };
        let encoded = payload
            .data
            .split_once(',')
            .map(|(_, data)| data)
            .ok_or_else(|| AppErrorDto {
                code: "mobile_file_error".to_string(),
                message: "the selected file could not be read".to_string(),
            })?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| AppErrorDto {
                code: "mobile_file_error".to_string(),
                message: error.to_string(),
            })?;
        Ok(Some(PickedFile {
            name: payload.name,
            bytes,
        }))
    }

    #[cfg(target_os = "ios")]
    async fn write_ios_document(
        existing_target: Option<String>,
        suggested_name: String,
        bytes: Vec<u8>,
        create_copy: bool,
    ) -> Result<Option<String>, AppErrorDto> {
        tokio::task::spawn_blocking(move || {
            let directory = dirs::document_dir().ok_or_else(|| AppErrorDto {
                code: "mobile_file_error".to_string(),
                message: "the iOS Documents directory is unavailable".to_string(),
            })?;
            std::fs::create_dir_all(&directory).map_err(io_error)?;
            let existing = existing_target.map(std::path::PathBuf::from);
            let mut target = existing
                .filter(|path| path.parent() == Some(directory.as_path()))
                .unwrap_or_else(|| directory.join(safe_file_name(&suggested_name)));
            if create_copy {
                target = unique_copy_path(target);
            }
            simple_table_engine::write_native_file_atomically(&target, &bytes)?;
            Ok(Some(target.to_string_lossy().into_owned()))
        })
        .await
        .map_err(|error| AppErrorDto {
            code: "mobile_file_error".to_string(),
            message: format!("iOS file task failed: {error}"),
        })?
    }

    fn eval_error(error: dioxus::document::EvalError) -> AppErrorDto {
        AppErrorDto {
            code: "mobile_file_error".to_string(),
            message: error.to_string(),
        }
    }

    #[cfg(target_os = "ios")]
    fn unique_copy_path(path: std::path::PathBuf) -> std::path::PathBuf {
        if !path.exists() {
            return path;
        }
        let parent = path.parent().unwrap_or_else(|| std::path::Path::new(""));
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("workbook");
        let extension = path.extension().and_then(|value| value.to_str());
        for index in 1..=9999 {
            let suffix = if index == 1 {
                " copy".to_string()
            } else {
                format!(" copy {index}")
            };
            let name = match extension {
                Some(extension) => format!("{stem}{suffix}.{extension}"),
                None => format!("{stem}{suffix}"),
            };
            let candidate = parent.join(name);
            if !candidate.exists() {
                return candidate;
            }
        }
        parent.join(format!("{stem} copy {}", uuid::Uuid::new_v4()))
    }

    #[cfg(target_os = "ios")]
    fn safe_file_name(name: &str) -> String {
        let name = std::path::Path::new(name)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("untitled.xlsx")
            .trim();
        if name.is_empty() {
            "untitled.xlsx".to_string()
        } else {
            name.to_string()
        }
    }

    #[cfg(target_os = "ios")]
    fn io_error(error: impl std::fmt::Display) -> AppErrorDto {
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
