#[cfg(any(target_os = "android", target_os = "ios"))]
use crate::error::AppError;
#[cfg(any(target_os = "android", target_os = "ios"))]
use crate::types::UpdateInfo;

/// Check for updates via GitHub Releases API (mobile only)
#[tauri::command(rename_all = "camelCase")]
#[cfg(any(target_os = "android", target_os = "ios"))]
pub async fn check_update_mobile(current_version: String) -> Result<Option<UpdateInfo>, AppError> {
    crate::adapters::update_adapter::check_mobile(current_version).await
}
