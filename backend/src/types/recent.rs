use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum StorageType {
    MobileSandboxPath,
    #[default]
    DesktopPath,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddRecentFileRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_path: Option<String>,
    #[serde(with = "super::u64_string")]
    pub document_id: u64,
    #[serde(with = "super::u64_string")]
    pub base_revision: u64,
    pub path: String,
    pub file_name: String,
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
            "path": "/managed/book.xlsx",
            "fileName": "book.xlsx",
            "originalPath": "/original/book.xlsx"
        }))
        .expect("recent request");

        assert_eq!(request.document_id, 7);
        assert_eq!(request.base_revision, 3);
        assert_eq!(request.path, "/managed/book.xlsx");
        assert_eq!(request.file_name, "book.xlsx");
        assert_eq!(
            request.original_path.as_deref(),
            Some("/original/book.xlsx")
        );
    }

    #[test]
    fn add_recent_request_requires_stable_file_identity() {
        let error = serde_json::from_value::<AddRecentFileRequest>(json!({
            "documentId": "7",
            "baseRevision": "3",
            "path": "/managed/book.xlsx"
        }))
        .expect_err("stable file identity should be required");

        assert!(error.to_string().contains("missing field"));
    }
}
