use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;

use crate::application::update_port::{UpdateRelease, UpdateReleaseAsset, UpdateReleasePort};
use crate::error::AppError;

const UPDATE_API_URL: &str = "https://api.github.com/repos/mingchiuli/simple-table/releases/latest";
const MAX_UPDATE_RESPONSE_BYTES: usize = 256 * 1024;
const UPDATE_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const UPDATE_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone)]
pub struct UpdateReleaseAdapter {
    client: Result<Client, String>,
    active: Arc<AtomicBool>,
}

impl Default for UpdateReleaseAdapter {
    fn default() -> Self {
        Self {
            client: Client::builder()
                .connect_timeout(UPDATE_CONNECT_TIMEOUT)
                .timeout(UPDATE_REQUEST_TIMEOUT)
                .build()
                .map_err(|error| error.to_string()),
            active: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl UpdateReleasePort for UpdateReleaseAdapter {
    fn latest_release(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<UpdateRelease, AppError>> + Send + '_>> {
        let client = self.client.clone();
        let active = Arc::clone(&self.active);
        Box::pin(async move {
            let _reservation = UpdateCheckReservation::try_begin(active)?;
            let client = client.map_err(|error| {
                AppError::Internal(format!("Failed to initialize update client: {error}"))
            })?;
            let response = client
                .get(UPDATE_API_URL)
                .header("User-Agent", "Simple-Table-App")
                .send()
                .await
                .map_err(|error| AppError::UpdateError(format!("request failed: {error}")))?;

            if !response.status().is_success() {
                return Err(AppError::UpdateError(format!(
                    "service returned {}",
                    response.status()
                )));
            }
            read_release_response(response).await
        })
    }
}

async fn read_release_response(mut response: reqwest::Response) -> Result<UpdateRelease, AppError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_UPDATE_RESPONSE_BYTES as u64)
    {
        return Err(update_response_limit_error());
    }

    let mut body = Vec::with_capacity(
        response
            .content_length()
            .unwrap_or_default()
            .min(MAX_UPDATE_RESPONSE_BYTES as u64) as usize,
    );
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| AppError::UpdateError(format!("failed to read response: {error}")))?
    {
        if body.len().saturating_add(chunk.len()) > MAX_UPDATE_RESPONSE_BYTES {
            return Err(update_response_limit_error());
        }
        body.extend_from_slice(&chunk);
    }
    decode_release_response(&body)
}

fn decode_release_response(body: &[u8]) -> Result<UpdateRelease, AppError> {
    if body.len() > MAX_UPDATE_RESPONSE_BYTES {
        return Err(update_response_limit_error());
    }
    let response: GitHubReleaseResponse = serde_json::from_slice(body)
        .map_err(|error| AppError::UpdateError(format!("invalid response: {error}")))?;
    Ok(UpdateRelease {
        tag_name: response.tag_name,
        assets: response
            .assets
            .into_iter()
            .map(|asset| UpdateReleaseAsset {
                name: asset.name,
                download_url: asset.browser_download_url,
            })
            .collect(),
    })
}

fn update_response_limit_error() -> AppError {
    AppError::ResourceLimitExceeded(format!(
        "update response exceeds {MAX_UPDATE_RESPONSE_BYTES} bytes"
    ))
}

#[derive(Deserialize)]
struct GitHubReleaseResponse {
    tag_name: String,
    assets: Vec<GitHubAssetResponse>,
}

#[derive(Deserialize)]
struct GitHubAssetResponse {
    name: String,
    browser_download_url: String,
}

struct UpdateCheckReservation {
    active: Arc<AtomicBool>,
}

impl UpdateCheckReservation {
    fn try_begin(active: Arc<AtomicBool>) -> Result<Self, AppError> {
        active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                AppError::ResourceLimitExceeded(
                    "an update check is already in progress".to_string(),
                )
            })?;
        Ok(Self { active })
    }
}

impl Drop for UpdateCheckReservation {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_response_has_a_hard_byte_limit() {
        let error = decode_release_response(&vec![b'x'; MAX_UPDATE_RESPONSE_BYTES + 1])
            .expect_err("oversized response");

        assert!(matches!(error, AppError::ResourceLimitExceeded(_)));
    }

    #[test]
    fn provider_response_is_mapped_to_an_internal_release() {
        let release = decode_release_response(
            br#"{
                "tag_name":"v1.2.3",
                "assets":[{"name":"app.apk","browser_download_url":"https://example.com/app.apk"}]
            }"#,
        )
        .expect("release response");

        assert_eq!(release.tag_name, "v1.2.3");
        assert_eq!(release.assets[0].name, "app.apk");
        assert_eq!(
            release.assets[0].download_url,
            "https://example.com/app.apk"
        );
    }

    #[test]
    fn update_check_admission_is_owned_per_adapter_runtime() {
        let first = UpdateReleaseAdapter::default();
        let second = UpdateReleaseAdapter::default();
        drop(first.latest_release());
        let reservation = UpdateCheckReservation::try_begin(Arc::clone(&first.active))
            .expect("first reservation");

        assert!(UpdateCheckReservation::try_begin(Arc::clone(&first.active)).is_err());
        assert!(UpdateCheckReservation::try_begin(Arc::clone(&second.active)).is_ok());
        drop(reservation);
        assert!(UpdateCheckReservation::try_begin(Arc::clone(&first.active)).is_ok());
    }
}
