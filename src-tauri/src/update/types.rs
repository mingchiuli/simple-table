use serde::{Deserialize, Serialize};

/// 更新信息，返回给前端
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub version: String,
    pub tag_name: String,
    pub release_url: String,
    pub apk_url: Option<String>,
}

/// GitHub Release API 响应结构
#[derive(Debug, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub assets: Vec<GitHubAsset>,
}

/// GitHub Release Asset 结构
#[derive(Debug, Deserialize)]
pub struct GitHubAsset {
    pub name: String,
    pub browser_download_url: String,
}