use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 文件存储类型
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum StorageType {
    /// 移动端官方 fs 插件管理的 App 沙盒路径
    MobileSandboxPath,
    /// 桌面端普通文件路径
    #[default]
    DesktopPath,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RecentFile {
    pub id: String,
    pub path: String,
    pub file_name: String,
    #[ts(type = "number")]
    pub last_opened: i64,
    #[ts(type = "number")]
    pub file_size: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub thumbnail: Option<String>,
    /// 存储类型（用于区分不同平台的文件处理方式）
    #[serde(default)]
    pub storage_type: StorageType,
    /// iOS: 原始文件路径（用于显示）
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub original_path: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AddRecentFileRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub original_path: Option<String>,
    #[serde(with = "crate::types::u64_string")]
    #[ts(type = "U64String")]
    pub document_id: u64,
    #[serde(with = "crate::types::u64_string")]
    #[ts(type = "U64String")]
    pub base_revision: u64,
}

impl RecentFile {
    pub fn new(path: String, file_name: String, file_size: i64) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            path,
            file_name,
            last_opened: timestamp_millis(SystemTime::now()),
            file_size,
            thumbnail: None,
            storage_type: StorageType::default(),
            original_path: None,
        }
    }
}

fn timestamp_millis(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn timestamp_millis_clamps_times_before_unix_epoch() {
        assert_eq!(timestamp_millis(UNIX_EPOCH - Duration::from_millis(1)), 0);
    }

    #[test]
    fn timestamp_millis_clamps_values_above_i64_range() {
        let far_future = UNIX_EPOCH + Duration::from_millis(i64::MAX as u64 + 1);

        assert_eq!(timestamp_millis(far_future), i64::MAX);
    }
}
