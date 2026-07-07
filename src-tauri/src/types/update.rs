use serde::{Deserialize, Serialize};

/// 更新信息，返回给前端
#[cfg(any(target_os = "android", target_os = "ios"))]
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub version: String,
    pub tag_name: String,
    pub release_url: String,
    pub apk_url: Option<String>,
}

/// GitHub Release API 响应结构
#[cfg(any(target_os = "android", target_os = "ios"))]
#[derive(Debug, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub assets: Vec<GitHubAsset>,
}

/// GitHub Release Asset 结构
#[cfg(any(target_os = "android", target_os = "ios"))]
#[derive(Debug, Deserialize)]
pub struct GitHubAsset {
    pub name: String,
    pub browser_download_url: String,
}
