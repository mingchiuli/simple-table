use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use crate::protocol::AppErrorDto;
#[cfg(not(any(feature = "server", feature = "mobile")))]
use serde::Deserialize;

#[cfg(not(any(feature = "server", feature = "mobile")))]
const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/mingchiuli/simple-table/releases/latest";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub version: String,
    pub url: String,
}

pub trait UpdatePort {
    fn check(&self) -> Pin<Box<dyn Future<Output = Result<Option<AvailableUpdate>, AppErrorDto>>>>;
}

pub struct GitHubUpdatePort;

pub fn platform_update_port() -> Rc<dyn UpdatePort> {
    Rc::new(GitHubUpdatePort)
}

#[cfg(not(any(feature = "server", feature = "mobile")))]
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
}

impl UpdatePort for GitHubUpdatePort {
    fn check(&self) -> Pin<Box<dyn Future<Output = Result<Option<AvailableUpdate>, AppErrorDto>>>> {
        Box::pin(async move {
            #[cfg(any(feature = "server", feature = "mobile"))]
            return Ok(None);

            #[cfg(not(any(feature = "server", feature = "mobile")))]
            {
                let release = fetch_release().await?;
                let latest = semver::Version::parse(release.tag_name.trim_start_matches('v'))
                    .map_err(update_error)?;
                let current =
                    semver::Version::parse(env!("CARGO_PKG_VERSION")).map_err(update_error)?;
                Ok((latest > current).then(|| AvailableUpdate {
                    version: latest.to_string(),
                    url: release.html_url,
                }))
            }
        })
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "desktop"))]
async fn fetch_release() -> Result<Release, AppErrorDto> {
    reqwest::Client::new()
        .get(LATEST_RELEASE_API)
        .header("User-Agent", "Simple-Table-App")
        .send()
        .await
        .map_err(update_error)?
        .error_for_status()
        .map_err(update_error)?
        .json()
        .await
        .map_err(update_error)
}

#[cfg(target_arch = "wasm32")]
async fn fetch_release() -> Result<Release, AppErrorDto> {
    use wasm_bindgen::JsCast;

    let window = web_sys::window().ok_or_else(|| AppErrorDto {
        code: "update_error".to_string(),
        message: "browser window is unavailable".to_string(),
    })?;
    let response = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(LATEST_RELEASE_API))
        .await
        .map_err(js_update_error)?
        .dyn_into::<web_sys::Response>()
        .map_err(js_update_error)?;
    let json = wasm_bindgen_futures::JsFuture::from(response.json().map_err(js_update_error)?)
        .await
        .map_err(js_update_error)?;
    serde_wasm_bindgen::from_value(json).map_err(update_error)
}

#[cfg(target_arch = "wasm32")]
fn js_update_error(error: wasm_bindgen::JsValue) -> AppErrorDto {
    AppErrorDto {
        code: "update_error".to_string(),
        message: error.as_string().unwrap_or_else(|| format!("{error:?}")),
    }
}

#[cfg(not(any(feature = "server", feature = "mobile")))]
fn update_error(error: impl std::fmt::Display) -> AppErrorDto {
    AppErrorDto {
        code: "update_error".to_string(),
        message: error.to_string(),
    }
}
