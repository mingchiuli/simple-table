#[cfg(any(target_os = "android", target_os = "ios"))]
use crate::error::AppError;
#[cfg(any(target_os = "android", target_os = "ios"))]
use crate::types::UpdateInfo;

#[cfg(any(target_os = "android", target_os = "ios"))]
pub async fn check_mobile(current_version: String) -> Result<Option<UpdateInfo>, AppError> {
    crate::update::check::check_update_mobile_impl(current_version).await
}
