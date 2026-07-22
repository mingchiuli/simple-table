use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum StorageType {
    MobileSandboxPath,
    #[default]
    DesktopPath,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
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
    #[serde(with = "super::u64_string")]
    #[ts(type = "U64String")]
    pub document_id: u64,
    #[serde(with = "super::u64_string")]
    #[ts(type = "U64String")]
    pub base_revision: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn add_recent_request_uses_string_document_context() {
        let request: AddRecentFileRequest = serde_json::from_value(json!({
            "documentId": "7",
            "baseRevision": "3",
            "originalPath": "/original/book.xlsx"
        }))
        .expect("recent request");

        assert_eq!(request.document_id, 7);
        assert_eq!(request.base_revision, 3);
        assert_eq!(
            request.original_path.as_deref(),
            Some("/original/book.xlsx")
        );
    }

    #[test]
    fn add_recent_request_requires_document_context() {
        let error = serde_json::from_value::<AddRecentFileRequest>(json!({
            "originalPath": "/original/book.xlsx"
        }))
        .expect_err("document context should be required");

        assert!(error.to_string().contains("missing field"));
    }
}
