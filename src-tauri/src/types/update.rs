#![cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 更新信息，返回给前端
#[derive(Debug, Serialize, Deserialize, TS, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub tag_name: String,
    pub release_url: String,
    pub apk_url: Option<String>,
}
