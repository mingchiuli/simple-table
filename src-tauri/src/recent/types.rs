use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// 文件存储类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum StorageType {
    /// 移动端官方 fs 插件管理的 App 沙盒路径
    MobileSandboxPath,
    /// 桌面端普通文件路径
    #[default]
    DesktopPath,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentFile {
    pub id: String,
    pub path: String,
    pub file_name: String,
    pub last_opened: i64,
    pub file_size: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    /// 存储类型（用于区分不同平台的文件处理方式）
    #[serde(default)]
    pub storage_type: StorageType,
    /// iOS: 原始文件路径（用于显示）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_path: Option<String>,
}

impl RecentFile {
    pub fn new(path: String, file_name: String, file_size: i64) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            path,
            file_name,
            last_opened: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
            file_size,
            thumbnail: None,
            storage_type: StorageType::default(),
            original_path: None,
        }
    }
}
