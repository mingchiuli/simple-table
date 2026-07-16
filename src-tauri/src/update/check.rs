#![cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use reqwest::Client;
use semver::Version;

use crate::error::AppError;
use crate::types::{GitHubAsset, GitHubRelease, UpdateInfo};

const UPDATE_API_URL: &str = "https://api.github.com/repos/mingchiuli/simple-table/releases/latest";
const RELEASE_URL_PREFIX: &str = "https://github.com/mingchiuli/simple-table/releases/tag/";
const APK_URL_PREFIX: &str = "https://github.com/mingchiuli/simple-table/releases/download/";
const MAX_UPDATE_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_VERSION_BYTES: usize = 128;
const UPDATE_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const UPDATE_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

static UPDATE_CHECK_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Check for updates via GitHub Releases API (mobile only).
pub async fn check_update_mobile_impl(
    current_version: String,
) -> Result<Option<UpdateInfo>, AppError> {
    let _reservation = UpdateCheckReservation::try_begin()?;
    let current_version = parse_version(&current_version, "current app version")?;
    let client = update_client()?;
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
    let release = read_release_response(response).await?;
    let release_version = parse_version(&release.tag_name, "release tag")?;
    if release_version <= current_version {
        return Ok(None);
    }

    Ok(Some(parse_release(release, release_version)))
}

async fn read_release_response(mut response: reqwest::Response) -> Result<GitHubRelease, AppError> {
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

fn decode_release_response(body: &[u8]) -> Result<GitHubRelease, AppError> {
    if body.len() > MAX_UPDATE_RESPONSE_BYTES {
        return Err(update_response_limit_error());
    }
    serde_json::from_slice(body)
        .map_err(|error| AppError::UpdateError(format!("invalid response: {error}")))
}

fn update_response_limit_error() -> AppError {
    AppError::ResourceLimitExceeded(format!(
        "update response exceeds {MAX_UPDATE_RESPONSE_BYTES} bytes"
    ))
}

fn parse_version(value: &str, label: &str) -> Result<Version, AppError> {
    if value.is_empty() || value.len() > MAX_VERSION_BYTES {
        return Err(AppError::UpdateError(format!(
            "Invalid {label}: expected between 1 and {MAX_VERSION_BYTES} bytes"
        )));
    }
    let normalized = value.strip_prefix('v').unwrap_or(value);
    Version::parse(normalized)
        .map_err(|error| AppError::UpdateError(format!("invalid {label} '{value}': {error}")))
}

fn find_apk_asset(assets: &[GitHubAsset]) -> Option<String> {
    assets
        .iter()
        .find(|asset| {
            asset.name.ends_with(".apk") && asset.browser_download_url.starts_with(APK_URL_PREFIX)
        })
        .map(|asset| asset.browser_download_url.clone())
}

fn parse_release(release: GitHubRelease, version: Version) -> UpdateInfo {
    let tag_name = release.tag_name;
    UpdateInfo {
        version: version.to_string(),
        release_url: format!("{RELEASE_URL_PREFIX}{tag_name}"),
        apk_url: find_apk_asset(&release.assets),
        tag_name,
    }
}

fn update_client() -> Result<&'static Client, AppError> {
    static UPDATE_CLIENT: OnceLock<Client> = OnceLock::new();
    if let Some(client) = UPDATE_CLIENT.get() {
        return Ok(client);
    }
    let client = Client::builder()
        .connect_timeout(UPDATE_CONNECT_TIMEOUT)
        .timeout(UPDATE_REQUEST_TIMEOUT)
        .build()
        .map_err(|error| {
            AppError::Internal(format!("Failed to initialize update client: {error}"))
        })?;
    Ok(UPDATE_CLIENT.get_or_init(|| client))
}

struct UpdateCheckReservation;

impl UpdateCheckReservation {
    fn try_begin() -> Result<Self, AppError> {
        UPDATE_CHECK_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                AppError::ResourceLimitExceeded(
                    "an update check is already in progress".to_string(),
                )
            })?;
        Ok(Self)
    }
}

impl Drop for UpdateCheckReservation {
    fn drop(&mut self) {
        UPDATE_CHECK_ACTIVE.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_comparison_handles_prereleases() {
        let current = parse_version("1.2.3-beta.1", "current").unwrap();
        let release = parse_version("v1.2.3", "release").unwrap();

        assert!(release > current);
        assert!(parse_version("1.2.2", "release").unwrap() < current);
    }

    #[test]
    fn invalid_versions_are_rejected_instead_of_partially_parsed() {
        assert!(parse_version("1.2.invalid", "release").is_err());
        assert!(parse_version("", "release").is_err());
    }

    #[test]
    fn release_response_has_a_hard_byte_limit() {
        let error = decode_release_response(&vec![b'x'; MAX_UPDATE_RESPONSE_BYTES + 1])
            .expect_err("oversized response");

        assert!(matches!(error, AppError::ResourceLimitExceeded(_)));
    }

    #[test]
    fn only_repository_apk_assets_are_exposed() {
        let release: GitHubRelease = serde_json::from_value(serde_json::json!({
            "tag_name": "v1.2.3",
            "assets": [
                { "name": "unsafe.apk", "browser_download_url": "https://example.com/unsafe.apk" },
                { "name": "simple-table.apk", "browser_download_url": "https://github.com/mingchiuli/simple-table/releases/download/v1.2.3/simple-table.apk" }
            ]
        }))
        .unwrap();

        let info = parse_release(release, Version::parse("1.2.3").unwrap());

        assert_eq!(
            info.apk_url.as_deref(),
            Some(
                "https://github.com/mingchiuli/simple-table/releases/download/v1.2.3/simple-table.apk"
            )
        );
    }
}
