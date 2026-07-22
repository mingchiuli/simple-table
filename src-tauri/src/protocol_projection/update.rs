#[cfg(any(target_os = "android", target_os = "ios"))]
use crate::projection_model::MobileUpdateSnapshot;
#[cfg(any(target_os = "android", target_os = "ios"))]
use crate::types;

#[cfg(any(target_os = "android", target_os = "ios"))]
pub(crate) fn mobile_update_info(value: MobileUpdateSnapshot) -> types::UpdateInfo {
    types::UpdateInfo {
        version: value.version,
        tag_name: value.tag_name,
        release_url: value.release_url,
        apk_url: value.apk_url,
    }
}
