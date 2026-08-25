mod workspace;

#[cfg(target_arch = "wasm32")]
mod indexed_db;

#[cfg(any(target_arch = "wasm32", test))]
use simple_table_protocol::AppErrorDto;
#[cfg(all(test, not(target_arch = "wasm32")))]
use simple_table_web_protocol::WEB_WORKER_PROTOCOL_VERSION;
#[cfg(any(target_arch = "wasm32", test))]
use simple_table_web_protocol::WorkerRequestEnvelope;
#[cfg(target_arch = "wasm32")]
use simple_table_web_protocol::{
    AttachmentMetadata, WEB_WORKER_PROTOCOL_VERSION, WorkerReply, WorkerResponseEnvelope,
};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
use crate::indexed_db::IndexedDbDocumentStore;
#[cfg(target_arch = "wasm32")]
use crate::workspace::WorkspaceService;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct WorkerSession {
    service: WorkspaceService<IndexedDbDocumentStore>,
}

#[cfg(target_arch = "wasm32")]
impl Default for WorkerSession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WorkerSession {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            service: WorkspaceService::new(IndexedDbDocumentStore::production()),
        }
    }

    pub async fn execute(
        &self,
        request_json: String,
        attachment: Option<js_sys::ArrayBuffer>,
    ) -> JsValue {
        let decoded = decode_request(
            &request_json,
            attachment.as_ref().map(js_sys::ArrayBuffer::byte_length),
        );
        let (message_id, response, response_attachment) = match decoded {
            Ok(envelope) => {
                let message_id = envelope.message_id;
                let attachment = attachment.map(|buffer| js_sys::Uint8Array::new(&buffer).to_vec());
                let (response, response_attachment) =
                    self.service.execute(envelope.request, attachment).await;
                (message_id, response, response_attachment)
            }
            Err(error) => (extract_message_id(&request_json), Err(error), None),
        };
        encode_response(message_id, response, response_attachment)
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn decode_request(
    request_json: &str,
    attachment_length: Option<u32>,
) -> Result<WorkerRequestEnvelope, AppErrorDto> {
    let envelope: WorkerRequestEnvelope =
        serde_json::from_str(request_json).map_err(|error| AppErrorDto {
            code: "invalid_request".to_string(),
            message: error.to_string(),
        })?;
    if envelope.protocol_version != WEB_WORKER_PROTOCOL_VERSION {
        return Err(worker_protocol_error(format!(
            "unsupported worker protocol version {}",
            envelope.protocol_version
        )));
    }
    let described_length = envelope
        .attachment
        .as_ref()
        .map(|metadata| metadata.byte_length);
    let actual_length = attachment_length.map(|length| length as usize);
    if described_length != actual_length {
        return Err(worker_protocol_error(
            "binary attachment metadata does not match the transferred buffer",
        ));
    }
    Ok(envelope)
}

#[cfg(any(target_arch = "wasm32", test))]
fn extract_message_id(request_json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(request_json)
        .ok()
        .and_then(|value| {
            value
                .get("messageId")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

#[cfg(target_arch = "wasm32")]
fn encode_response(
    message_id: String,
    response: Result<WorkerReply, AppErrorDto>,
    attachment: Option<Vec<u8>>,
) -> JsValue {
    use js_sys::{Object, Reflect, Uint8Array};

    let metadata = WorkerResponseEnvelope {
        protocol_version: WEB_WORKER_PROTOCOL_VERSION,
        message_id,
        response,
        attachment: attachment.as_ref().map(|bytes| AttachmentMetadata {
            byte_length: bytes.len(),
        }),
    };
    let metadata = serde_json::to_string(&metadata).unwrap_or_else(|error| {
        format!(
            r#"{{"protocolVersion":{},"messageId":"","response":{{"Err":{{"code":"serialization_error","message":{}}}}}}}"#,
            WEB_WORKER_PROTOCOL_VERSION,
            serde_json::to_string(&error.to_string()).unwrap_or_else(|_| "null".to_string())
        )
    });
    let value = Object::new();
    Reflect::set(&value, &"metadata".into(), &metadata.into())
        .expect("worker response metadata property");
    if let Some(bytes) = attachment {
        let buffer = Uint8Array::from(bytes.as_slice()).buffer();
        Reflect::set(&value, &"attachment".into(), &buffer)
            .expect("worker response attachment property");
    }
    value.into()
}

#[cfg(any(target_arch = "wasm32", test))]
fn worker_protocol_error(message: impl Into<String>) -> AppErrorDto {
    AppErrorDto {
        code: "worker_protocol_error".to_string(),
        message: message.into(),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use simple_table_protocol::EditorRequest;
    use simple_table_web_protocol::{AttachmentMetadata, WorkerRequest};

    use super::*;

    fn request_json(attachment: Option<AttachmentMetadata>) -> String {
        serde_json::to_string(&WorkerRequestEnvelope {
            protocol_version: WEB_WORKER_PROTOCOL_VERSION,
            message_id: "message-7".to_string(),
            request: WorkerRequest::Editor(EditorRequest::OpenDocument {
                request_id: "open-7".to_string(),
                file_name: "book.xlsx".to_string(),
            }),
            attachment,
        })
        .expect("worker request should serialize")
    }

    #[test]
    fn request_attachment_length_must_match_the_transferred_buffer() {
        let json = request_json(Some(AttachmentMetadata { byte_length: 3 }));

        let error = decode_request(&json, Some(2)).expect_err("mismatch must fail");

        assert_eq!(error.code, "worker_protocol_error");
    }

    #[test]
    fn request_protocol_version_is_validated_before_dispatch() {
        let json = request_json(None).replace(
            &format!("\"protocolVersion\":{WEB_WORKER_PROTOCOL_VERSION}"),
            "\"protocolVersion\":999",
        );

        let error = decode_request(&json, None).expect_err("unknown version must fail");

        assert_eq!(error.code, "worker_protocol_error");
    }

    #[test]
    fn message_id_survives_request_decode_errors() {
        assert_eq!(extract_message_id(&request_json(None)), "message-7");
    }
}
