#[cfg(any(target_os = "android", target_os = "ios"))]
use crate::error::AppError;
#[cfg(any(target_os = "android", target_os = "ios"))]
use crate::protocol_projection;
#[cfg(any(target_os = "android", target_os = "ios"))]
use crate::runtime::ApplicationRuntime;
#[cfg(any(target_os = "android", target_os = "ios"))]
use crate::types::UpdateInfo;
#[cfg(any(target_os = "android", target_os = "ios"))]
use tauri::State;

/// Check for updates via GitHub Releases API (mobile only)
#[tauri::command(rename_all = "camelCase")]
#[cfg(any(target_os = "android", target_os = "ios"))]
pub async fn check_update_mobile(
    runtime: State<'_, ApplicationRuntime>,
    current_version: String,
) -> Result<Option<UpdateInfo>, AppError> {
    let service = runtime.update_queries().clone();
    service
        .check_mobile(&current_version)
        .await
        .map(|update| update.map(protocol_projection::mobile_update_info))
}
