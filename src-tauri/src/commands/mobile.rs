#[cfg(any(target_os = "android", target_os = "ios"))]
use crate::types::UpdateInfo;
#[cfg(any(target_os = "android", target_os = "ios"))]
use crate::update::check::check_update_mobile_impl;

/// Check for updates via GitHub Releases API (mobile only)
#[tauri::command(rename_all = "camelCase")]
#[cfg(any(target_os = "android", target_os = "ios"))]
pub async fn check_update_mobile(current_version: String) -> Result<Option<UpdateInfo>, String> {
    check_update_mobile_impl(current_version).await
}
