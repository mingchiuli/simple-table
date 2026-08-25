use serde::{Deserialize, Serialize};
use simple_table_protocol::{
    AppErrorDto, EditorReply, EditorRequest, OpenDocumentResponse, SavedDocumentResponse,
};

pub const WEB_WORKER_PROTOCOL_VERSION: u16 = 1;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentMetadata {
    pub byte_length: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum WorkerRequest {
    Editor(EditorRequest),
    Workspace(WebWorkspaceRequest),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum WorkerReply {
    Editor(EditorReply),
    Workspace(WebWorkspaceReply),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WebWorkspaceRequest {
    SaveLocal {
        request_id: String,
        document_id: u64,
        base_revision: u64,
        target_name: String,
    },
    CheckpointRecovery {
        request_id: String,
        document_id: u64,
        base_revision: u64,
        target_name: String,
    },
    ClearRecovery,
    ListLocalDocuments,
    OpenLocalDocument {
        request_id: String,
        document_key: String,
    },
    DeleteLocalDocument {
        document_key: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalDocumentSummary {
    pub id: String,
    pub name: String,
    pub updated_at_ms: u64,
    pub has_recovery: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum WebWorkspaceReply {
    Empty,
    Document(OpenDocumentResponse),
    Saved(SavedDocumentResponse),
    LocalDocuments(Vec<LocalDocumentSummary>),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkerRequestEnvelope {
    pub protocol_version: u16,
    pub message_id: String,
    pub request: WorkerRequest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment: Option<AttachmentMetadata>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkerResponseEnvelope {
    pub protocol_version: u16,
    pub message_id: String,
    pub response: Result<WorkerReply, AppErrorDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment: Option<AttachmentMetadata>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_envelope_describes_binary_without_embedding_it_in_json() {
        let envelope = WorkerRequestEnvelope {
            protocol_version: WEB_WORKER_PROTOCOL_VERSION,
            message_id: "message-1".to_string(),
            request: WorkerRequest::Editor(EditorRequest::OpenDocument {
                request_id: "open-1".to_string(),
                file_name: "workbook.xlsx".to_string(),
            }),
            attachment: Some(AttachmentMetadata { byte_length: 4 }),
        };

        let json = serde_json::to_string(&envelope).expect("serialize worker envelope");
        let decoded: WorkerRequestEnvelope =
            serde_json::from_str(&json).expect("deserialize worker envelope");

        assert_eq!(decoded, envelope);
        assert!(json.contains("\"byteLength\":4"));
        assert!(!json.contains("AQIDBA"));
    }

    #[test]
    fn message_ids_round_trip_on_error_responses() {
        let envelope = WorkerResponseEnvelope {
            protocol_version: WEB_WORKER_PROTOCOL_VERSION,
            message_id: "message-2".to_string(),
            response: Err(AppErrorDto {
                code: "invalid_request".to_string(),
                message: "bad request".to_string(),
            }),
            attachment: None,
        };

        let json = serde_json::to_string(&envelope).expect("serialize worker response");
        let decoded: WorkerResponseEnvelope =
            serde_json::from_str(&json).expect("deserialize worker response");

        assert_eq!(decoded, envelope);
    }
}
