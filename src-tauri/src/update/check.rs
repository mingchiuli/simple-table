use std::cmp::max;

use crate::types::{GitHubAsset, GitHubRelease, UpdateInfo};
use reqwest::Client;

/// Check for updates via GitHub Releases API (mobile only)
#[cfg(any(target_os = "android", target_os = "ios"))]
pub async fn check_update_mobile_impl(
    current_version: String,
) -> Result<Option<UpdateInfo>, String> {
    let client = Client::new();

    let response = client
        .get("https://api.github.com/repos/mingchiuli/simple-table/releases/latest")
        .header("User-Agent", "Simple-Table-App")
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("GitHub API error: {}", response.status()));
    }

    let release: GitHubRelease = response
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name)
        .to_string();

    if !is_newer_version(&version, &current_version) {
        return Ok(None);
    }

    Ok(Some(parse_release(release)))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn is_newer_version(new: &str, current: &str) -> bool {
    let new_parts: Vec<u32> = new.split('.').filter_map(|s| s.parse().ok()).collect();
    let current_parts: Vec<u32> = current.split('.').filter_map(|s| s.parse().ok()).collect();

    for i in 0..max(new_parts.len(), current_parts.len()) {
        let new_val = new_parts.get(i).unwrap_or(&0);
        let current_val = current_parts.get(i).unwrap_or(&0);
        if new_val > current_val {
            return true;
        }
        if new_val < current_val {
            return false;
        }
    }
    false
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn find_apk_asset(assets: &[GitHubAsset]) -> Option<String> {
    assets
        .iter()
        .find(|a| a.name.ends_with(".apk"))
        .map(|a| a.browser_download_url.clone())
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn parse_release(release: GitHubRelease) -> UpdateInfo {
    let tag_name = release.tag_name;
    let version = tag_name.strip_prefix('v').unwrap_or(&tag_name).to_string();
    let apk_url = find_apk_asset(&release.assets);
    let release_url = format!(
        "https://github.com/mingchiuli/simple-table/releases/tag/{}",
        tag_name
    );

    UpdateInfo {
        version,
        tag_name,
        release_url,
        apk_url,
    }
}
