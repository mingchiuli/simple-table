#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;

#[cfg(target_arch = "wasm32")]
use simple_table_engine::CoreFacade;
#[cfg(any(target_arch = "wasm32", test))]
use simple_table_protocol::AppErrorDto;
#[cfg(target_arch = "wasm32")]
use simple_table_protocol::{EditorCommand, EditorOutput};
#[cfg(all(test, not(target_arch = "wasm32")))]
use simple_table_web_protocol::WEB_WORKER_PROTOCOL_VERSION;
#[cfg(any(target_arch = "wasm32", test))]
use simple_table_web_protocol::WorkerRequestEnvelope;
#[cfg(target_arch = "wasm32")]
use simple_table_web_protocol::{
    AttachmentMetadata, WEB_WORKER_PROTOCOL_VERSION, WorkerReply, WorkerRequest,
    WorkerResponseEnvelope,
};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(any(target_arch = "wasm32", test))]
const STORED_DOCUMENT_SCHEMA_VERSION: u16 = 2;

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredDocument {
    #[serde(default = "legacy_schema_version")]
    schema_version: u16,
    id: String,
    name: String,
    #[serde(with = "serde_bytes")]
    saved_bytes: Vec<u8>,
    #[serde(default, with = "option_bytes")]
    recovery_bytes: Option<Vec<u8>>,
    saved_content_hash: String,
    #[serde(default, with = "option_bytes")]
    thumbnail: Option<Vec<u8>>,
    updated_at_ms: u64,
}

#[cfg(any(target_arch = "wasm32", test))]
fn legacy_schema_version() -> u16 {
    1
}

#[cfg(any(target_arch = "wasm32", test))]
mod option_bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use serde_bytes::{ByteBuf, Bytes};

    pub fn serialize<S>(value: &Option<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value.as_deref().map(Bytes::new).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<u8>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<ByteBuf>::deserialize(deserializer).map(|value| value.map(ByteBuf::into_vec))
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct WorkerSession {
    core: RefCell<CoreFacade>,
    #[cfg(target_arch = "wasm32")]
    active_document_key: RefCell<Option<String>>,
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
            core: RefCell::new(CoreFacade::default()),
            #[cfg(target_arch = "wasm32")]
            active_document_key: RefCell::new(None),
        }
    }

    #[cfg(target_arch = "wasm32")]
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
                    self.execute_web(envelope.request, attachment).await;
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

#[cfg(test)]
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

    #[test]
    fn legacy_stored_documents_keep_their_workbook_and_recovery_bytes() {
        let legacy = r#"{
            "id":"document-1",
            "name":"legacy.xlsx",
            "savedBytes":[1,2,3],
            "recoveryBytes":[4,5],
            "savedContentHash":"hash",
            "thumbnail":null,
            "updatedAtMs":42
        }"#;

        let document: StoredDocument =
            serde_json::from_str(legacy).expect("legacy record should deserialize");

        assert_eq!(document.schema_version, 1);
        assert_eq!(document.saved_bytes, vec![1, 2, 3]);
        assert_eq!(document.recovery_bytes, Some(vec![4, 5]));
        assert!(document.schema_version < STORED_DOCUMENT_SCHEMA_VERSION);
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use rexie::{ObjectStore, Rexie, TransactionMode};
    use sha2::{Digest, Sha256};
    use simple_table_protocol::{EditorReply, EditorRequest};
    use simple_table_web_protocol::{LocalDocumentSummary, WebWorkspaceReply, WebWorkspaceRequest};
    use wasm_bindgen::JsValue;

    use super::*;

    const DATABASE_NAME: &str = "simple-table";
    const DOCUMENTS_STORE: &str = "documents";
    const SETTINGS_STORE: &str = "settings";
    impl WorkerSession {
        pub(super) async fn execute_web(
            &self,
            request: WorkerRequest,
            attachment: Option<Vec<u8>>,
        ) -> (Result<WorkerReply, AppErrorDto>, Option<Vec<u8>>) {
            let result = match request {
                WorkerRequest::Editor(request) => self.execute_editor(request, attachment).await,
                WorkerRequest::Workspace(request) => {
                    if attachment.is_some() {
                        Err(worker_protocol_error(
                            "workspace requests do not accept binary attachments",
                        ))
                    } else {
                        self.execute_workspace(request).await
                    }
                }
            };
            match result {
                Ok((reply, attachment)) => (Ok(reply), attachment),
                Err(error) => (Err(error), None),
            }
        }

        async fn execute_editor(
            &self,
            request: EditorRequest,
            attachment: Option<Vec<u8>>,
        ) -> Result<(WorkerReply, Option<Vec<u8>>), AppErrorDto> {
            let replaces_document = matches!(
                request,
                EditorRequest::NewDocument { .. }
                    | EditorRequest::OpenDocument { .. }
                    | EditorRequest::OpenRecoveryDocument { .. }
            );
            let closes_document = matches!(request, EditorRequest::CloseDocument { .. });
            let previous_key = (replaces_document || closes_document)
                .then(|| self.active_document_key.borrow().clone())
                .flatten();
            if closes_document && let Some(document_key) = previous_key.as_deref() {
                clear_recovery(document_key).await?;
            }
            let EditorOutput { reply, attachment } =
                self.core.borrow_mut().execute(EditorCommand {
                    request,
                    attachment,
                })?;
            if replaces_document {
                if let Some(document_key) = previous_key {
                    let _ = clear_recovery(&document_key).await;
                }
                self.active_document_key.replace(None);
            } else if closes_document {
                self.active_document_key.replace(None);
            }
            Ok((WorkerReply::Editor(reply), attachment))
        }

        async fn execute_workspace(
            &self,
            request: WebWorkspaceRequest,
        ) -> Result<(WorkerReply, Option<Vec<u8>>), AppErrorDto> {
            match request {
                WebWorkspaceRequest::SaveLocal {
                    request_id,
                    document_id,
                    base_revision,
                    target_name,
                } => {
                    self.save_local(request_id, document_id, base_revision, target_name)
                        .await
                }
                WebWorkspaceRequest::CheckpointRecovery {
                    request_id,
                    document_id,
                    base_revision,
                    target_name,
                } => {
                    self.checkpoint_recovery(request_id, document_id, base_revision, target_name)
                        .await
                }
                WebWorkspaceRequest::ClearRecovery => {
                    let document_key = self.active_document_key.borrow().clone();
                    if let Some(document_key) = document_key
                        && clear_recovery(&document_key).await?
                    {
                        self.active_document_key.replace(None);
                    }
                    Ok((WorkerReply::Workspace(WebWorkspaceReply::Empty), None))
                }
                WebWorkspaceRequest::ListLocalDocuments => Ok((
                    WorkerReply::Workspace(WebWorkspaceReply::LocalDocuments(
                        list_documents().await?,
                    )),
                    None,
                )),
                WebWorkspaceRequest::OpenLocalDocument {
                    request_id,
                    document_key,
                } => self.open_local(request_id, document_key).await,
                WebWorkspaceRequest::DeleteLocalDocument { document_key } => {
                    delete_document(&document_key).await?;
                    if self.active_document_key.borrow().as_deref() == Some(&document_key) {
                        self.active_document_key.replace(None);
                    }
                    Ok((WorkerReply::Workspace(WebWorkspaceReply::Empty), None))
                }
            }
        }

        async fn save_local(
            &self,
            request_id: String,
            document_id: u64,
            base_revision: u64,
            target_name: String,
        ) -> Result<(WorkerReply, Option<Vec<u8>>), AppErrorDto> {
            let EditorOutput {
                reply: prepared,
                attachment,
            } = self.core.borrow_mut().execute(EditorRequest::PrepareSave {
                request_id: request_id.clone(),
                document_id,
                base_revision,
                target_name: target_name.clone(),
            })?;
            let EditorReply::SavePrepared {
                save_token,
                file_name,
            } = prepared
            else {
                return Err(worker_error(
                    "unexpected_reply",
                    "core did not prepare a save",
                ));
            };
            let bytes = attachment.ok_or_else(|| {
                worker_error("unexpected_reply", "core omitted prepared save bytes")
            })?;

            let document_key = self
                .active_document_key
                .borrow()
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let stored = StoredDocument {
                schema_version: STORED_DOCUMENT_SCHEMA_VERSION,
                id: document_key.clone(),
                name: file_name,
                saved_content_hash: content_hash(&bytes),
                saved_bytes: bytes,
                recovery_bytes: None,
                thumbnail: None,
                updated_at_ms: now_ms(),
            };
            if let Err(error) = put_document(&stored).await {
                let _ = self
                    .core
                    .borrow_mut()
                    .execute(EditorRequest::AbortSave { save_token });
                return Err(error);
            }
            let EditorOutput { reply, attachment } =
                self.core.borrow_mut().execute(EditorRequest::CommitSave {
                    save_token,
                    path: format!("indexeddb://{document_key}/{target_name}"),
                })?;
            let EditorReply::Saved { value } = reply else {
                return Err(worker_error(
                    "unexpected_reply",
                    "core did not commit the local save",
                ));
            };
            self.active_document_key.replace(Some(document_key));
            Ok((
                WorkerReply::Workspace(WebWorkspaceReply::Saved(value)),
                attachment,
            ))
        }

        async fn checkpoint_recovery(
            &self,
            request_id: String,
            document_id: u64,
            base_revision: u64,
            target_name: String,
        ) -> Result<(WorkerReply, Option<Vec<u8>>), AppErrorDto> {
            let EditorOutput {
                reply: prepared,
                attachment,
            } = self.core.borrow_mut().execute(EditorRequest::PrepareSave {
                request_id,
                document_id,
                base_revision,
                target_name: target_name.clone(),
            })?;
            let EditorReply::SavePrepared {
                save_token,
                file_name,
            } = prepared
            else {
                return Err(worker_error(
                    "unexpected_reply",
                    "core did not prepare recovery bytes",
                ));
            };
            let bytes = attachment
                .ok_or_else(|| worker_error("unexpected_reply", "core omitted recovery bytes"))?;
            let _ = self
                .core
                .borrow_mut()
                .execute(EditorRequest::AbortSave { save_token });

            let document_key = self
                .active_document_key
                .borrow()
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let mut stored = get_document(&document_key)
                .await?
                .unwrap_or(StoredDocument {
                    schema_version: STORED_DOCUMENT_SCHEMA_VERSION,
                    id: document_key.clone(),
                    name: file_name,
                    saved_bytes: Vec::new(),
                    recovery_bytes: None,
                    saved_content_hash: String::new(),
                    thumbnail: None,
                    updated_at_ms: now_ms(),
                });
            stored.recovery_bytes = Some(bytes);
            stored.updated_at_ms = now_ms();
            put_document(&stored).await?;
            self.active_document_key.replace(Some(document_key));
            Ok((WorkerReply::Workspace(WebWorkspaceReply::Empty), None))
        }

        async fn open_local(
            &self,
            request_id: String,
            document_key: String,
        ) -> Result<(WorkerReply, Option<Vec<u8>>), AppErrorDto> {
            let stored = get_document(&document_key)
                .await?
                .ok_or_else(|| worker_error("not_found", "local document no longer exists"))?;
            let recovery = stored
                .recovery_bytes
                .clone()
                .filter(|bytes| !bytes.is_empty());
            let recovered = recovery.is_some();
            let bytes = recovery.unwrap_or(stored.saved_bytes);
            if bytes.is_empty() {
                return Err(worker_error(
                    "not_found",
                    "local document has no recoverable data",
                ));
            }
            let request = if recovered {
                EditorRequest::OpenRecoveryDocument {
                    request_id,
                    file_name: stored.name,
                }
            } else {
                EditorRequest::OpenDocument {
                    request_id,
                    file_name: stored.name,
                }
            };
            let EditorOutput { reply, attachment } = self
                .core
                .borrow_mut()
                .execute(EditorCommand::with_attachment(request, bytes))?;
            let EditorReply::Document { value: Some(value) } = reply else {
                return Err(worker_error(
                    "unexpected_reply",
                    "core did not open the local workbook",
                ));
            };
            self.active_document_key.replace(Some(document_key));
            Ok((
                WorkerReply::Workspace(WebWorkspaceReply::Document(value)),
                attachment,
            ))
        }
    }

    async fn database() -> Result<Rexie, AppErrorDto> {
        Rexie::builder(DATABASE_NAME)
            .version(2)
            .add_object_store(ObjectStore::new(DOCUMENTS_STORE).key_path("id"))
            .add_object_store(ObjectStore::new(SETTINGS_STORE).key_path("key"))
            .build()
            .await
            .map_err(indexed_db_error)
    }

    async fn put_document(document: &StoredDocument) -> Result<(), AppErrorDto> {
        let database = database().await?;
        let transaction = database
            .transaction(&[DOCUMENTS_STORE], TransactionMode::ReadWrite)
            .map_err(indexed_db_error)?;
        let store = transaction
            .store(DOCUMENTS_STORE)
            .map_err(indexed_db_error)?;
        let mut document = document.clone();
        document.schema_version = STORED_DOCUMENT_SCHEMA_VERSION;
        let value = serde_wasm_bindgen::to_value(&document).map_err(js_serde_error)?;
        store.put(&value, None).await.map_err(indexed_db_error)?;
        transaction.done().await.map_err(indexed_db_error)?;
        Ok(())
    }

    async fn get_document(document_key: &str) -> Result<Option<StoredDocument>, AppErrorDto> {
        let database = database().await?;
        let transaction = database
            .transaction(&[DOCUMENTS_STORE], TransactionMode::ReadOnly)
            .map_err(indexed_db_error)?;
        let store = transaction
            .store(DOCUMENTS_STORE)
            .map_err(indexed_db_error)?;
        let document: Option<StoredDocument> = store
            .get(JsValue::from_str(document_key))
            .await
            .map_err(indexed_db_error)?
            .map(|value| serde_wasm_bindgen::from_value(value).map_err(js_serde_error))
            .transpose()?;
        transaction.done().await.map_err(indexed_db_error)?;
        if let Some(document) = document.as_ref()
            && document.schema_version < STORED_DOCUMENT_SCHEMA_VERSION
        {
            let _ = put_document(document).await;
        }
        Ok(document)
    }

    async fn list_documents() -> Result<Vec<LocalDocumentSummary>, AppErrorDto> {
        let database = database().await?;
        let transaction = database
            .transaction(&[DOCUMENTS_STORE], TransactionMode::ReadOnly)
            .map_err(indexed_db_error)?;
        let store = transaction
            .store(DOCUMENTS_STORE)
            .map_err(indexed_db_error)?;
        let mut documents = store
            .get_all(None, None)
            .await
            .map_err(indexed_db_error)?
            .into_iter()
            .map(|value| {
                serde_wasm_bindgen::from_value::<StoredDocument>(value).map_err(js_serde_error)
            })
            .collect::<Result<Vec<_>, _>>()?;
        transaction.done().await.map_err(indexed_db_error)?;
        for document in documents
            .iter()
            .filter(|document| document.schema_version < STORED_DOCUMENT_SCHEMA_VERSION)
        {
            let _ = put_document(document).await;
        }
        documents.sort_by_key(|document| std::cmp::Reverse(document.updated_at_ms));
        Ok(documents
            .into_iter()
            .map(|document| LocalDocumentSummary {
                id: document.id,
                name: document.name,
                updated_at_ms: document.updated_at_ms,
                has_recovery: document.recovery_bytes.is_some(),
            })
            .collect())
    }

    async fn delete_document(document_key: &str) -> Result<(), AppErrorDto> {
        let database = database().await?;
        let transaction = database
            .transaction(&[DOCUMENTS_STORE], TransactionMode::ReadWrite)
            .map_err(indexed_db_error)?;
        let store = transaction
            .store(DOCUMENTS_STORE)
            .map_err(indexed_db_error)?;
        store
            .delete(JsValue::from_str(document_key))
            .await
            .map_err(indexed_db_error)?;
        transaction.done().await.map_err(indexed_db_error)?;
        Ok(())
    }

    async fn clear_recovery(document_key: &str) -> Result<bool, AppErrorDto> {
        let Some(mut document) = get_document(document_key).await? else {
            return Ok(true);
        };
        if document.saved_bytes.is_empty() {
            delete_document(document_key).await?;
            return Ok(true);
        }
        if document.recovery_bytes.take().is_some() {
            put_document(&document).await?;
        }
        Ok(false)
    }

    fn content_hash(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn now_ms() -> u64 {
        js_sys::Date::now().max(0.0) as u64
    }

    fn indexed_db_error(error: impl std::fmt::Display) -> AppErrorDto {
        worker_error("indexed_db_error", &error.to_string())
    }

    fn js_serde_error(error: impl std::fmt::Display) -> AppErrorDto {
        worker_error("indexed_db_serialization_error", &error.to_string())
    }

    fn worker_error(code: &str, message: &str) -> AppErrorDto {
        AppErrorDto {
            code: code.to_string(),
            message: message.to_string(),
        }
    }
}
