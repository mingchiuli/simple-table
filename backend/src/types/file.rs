use serde::{Deserialize, Serialize};

use super::document::DocumentManifest;
use super::editor_session::EditorSessionInfo;

#[cfg(any(target_os = "android", target_os = "ios"))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedFileInfo {
    pub path: String,
    pub original_path: String,
    pub file_name: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FileOperationKind {
    Open,
    Close,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileOperationReceipt {
    pub kind: FileOperationKind,
    #[serde(with = "crate::types::u64_string")]
    pub document_id: u64,
    #[serde(with = "crate::types::u64_string")]
    pub revision: u64,
    pub path: String,
    pub file_name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SavedDocumentIdentity {
    pub path: String,
    pub file_name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SavedDocumentResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<DocumentManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<SavedDocumentIdentity>,
    pub editor_session: EditorSessionInfo,
}
