use serde::{Deserialize, Serialize};
use ts_rs::TS;

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

#[cfg(desktop)]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopOpenFileInfo {
    pub path: String,
    pub file_name: String,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PreparedOpenDocument {
    pub token: String,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SavedDocumentIdentity {
    pub path: String,
    pub file_name: String,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SavedDocumentResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub document: Option<DocumentManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub identity: Option<SavedDocumentIdentity>,
    pub editor_session: EditorSessionInfo,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SpreadsheetFormatOptions {
    pub default_extension: String,
    pub supported_extensions: Vec<String>,
}
