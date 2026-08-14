use std::cell::RefCell;

use simple_table_engine::CoreFacade;
use simple_table_protocol::{AppErrorDto, EditorRequest, EditorResponse};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct WorkerSession {
    core: RefCell<CoreFacade>,
    #[cfg(target_arch = "wasm32")]
    active_document_key: RefCell<Option<String>>,
}

impl Default for WorkerSession {
    fn default() -> Self {
        Self::new()
    }
}

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

    #[cfg(not(target_arch = "wasm32"))]
    pub fn execute(&self, request_json: &str) -> String {
        let response = decode_request(request_json)
            .and_then(|request| self.core.borrow_mut().execute(request));
        encode_response(response)
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn execute(&self, request_json: String) -> String {
        let response = match decode_request(&request_json) {
            Ok(request) => self.execute_web(request).await,
            Err(error) => Err(error),
        };
        encode_response(response)
    }
}

fn decode_request(request_json: &str) -> Result<EditorRequest, AppErrorDto> {
    serde_json::from_str(request_json).map_err(|error| AppErrorDto {
        code: "invalid_request".to_string(),
        message: error.to_string(),
    })
}

fn encode_response(response: EditorResponse) -> String {
    serde_json::to_string(&response).unwrap_or_else(|error| {
        format!(
            r#"{{"Err":{{"code":"serialization_error","message":{}}}}}"#,
            serde_json::to_string(&error.to_string()).unwrap_or_else(|_| "null".to_string())
        )
    })
}

#[cfg(target_arch = "wasm32")]
mod web {
    use rexie::{ObjectStore, Rexie, TransactionMode};
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256};
    use simple_table_protocol::{EditorReply, LocalDocumentSummary};
    use wasm_bindgen::JsValue;

    use super::*;

    const DATABASE_NAME: &str = "simple-table";
    const DOCUMENTS_STORE: &str = "documents";
    const SETTINGS_STORE: &str = "settings";

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct StoredDocument {
        id: String,
        name: String,
        saved_bytes: Vec<u8>,
        recovery_bytes: Option<Vec<u8>>,
        saved_content_hash: String,
        thumbnail: Option<Vec<u8>>,
        updated_at_ms: u64,
    }

    impl WorkerSession {
        pub(super) async fn execute_web(&self, request: EditorRequest) -> EditorResponse {
            match request {
                request @ (EditorRequest::NewDocument { .. }
                | EditorRequest::OpenDocument { .. }) => {
                    let previous_key = self.active_document_key.borrow().clone();
                    let response = self.core.borrow_mut().execute(request);
                    if response.is_ok() {
                        if let Some(document_key) = previous_key {
                            let _ = clear_recovery(&document_key).await;
                        }
                        self.active_document_key.replace(None);
                    }
                    response
                }
                EditorRequest::SaveLocal {
                    request_id,
                    document_id,
                    base_revision,
                    target_name,
                } => {
                    self.save_local(request_id, document_id, base_revision, target_name)
                        .await
                }
                EditorRequest::CheckpointRecovery {
                    request_id,
                    document_id,
                    base_revision,
                    target_name,
                } => {
                    self.checkpoint_recovery(request_id, document_id, base_revision, target_name)
                        .await
                }
                EditorRequest::ClearRecovery => {
                    let document_key = self.active_document_key.borrow().clone();
                    if let Some(document_key) = document_key
                        && clear_recovery(&document_key).await?
                    {
                        self.active_document_key.replace(None);
                    }
                    Ok(EditorReply::Empty)
                }
                EditorRequest::ListLocalDocuments => list_documents().await,
                EditorRequest::OpenLocalDocument {
                    request_id,
                    document_key,
                } => self.open_local(request_id, document_key).await,
                EditorRequest::DeleteLocalDocument { document_key } => {
                    delete_document(&document_key).await?;
                    if self.active_document_key.borrow().as_deref() == Some(&document_key) {
                        self.active_document_key.replace(None);
                    }
                    Ok(EditorReply::Empty)
                }
                request @ EditorRequest::CloseDocument { .. } => {
                    let document_key = self.active_document_key.borrow().clone();
                    if let Some(document_key) = document_key {
                        clear_recovery(&document_key).await?;
                    }
                    let response = self.core.borrow_mut().execute(request);
                    if response.is_ok() {
                        self.active_document_key.replace(None);
                    }
                    response
                }
                request => self.core.borrow_mut().execute(request),
            }
        }

        async fn save_local(
            &self,
            request_id: String,
            document_id: u64,
            base_revision: u64,
            target_name: String,
        ) -> EditorResponse {
            let prepared = self.core.borrow_mut().execute(EditorRequest::PrepareSave {
                request_id: request_id.clone(),
                document_id,
                base_revision,
                target_name: target_name.clone(),
            })?;
            let EditorReply::SavePrepared {
                save_token,
                file_name,
                bytes,
            } = prepared
            else {
                return Err(worker_error(
                    "unexpected_reply",
                    "core did not prepare a save",
                ));
            };

            let document_key = self
                .active_document_key
                .borrow()
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let stored = StoredDocument {
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
            let response = self.core.borrow_mut().execute(EditorRequest::CommitSave {
                save_token,
                path: format!("indexeddb://{document_key}/{target_name}"),
            });
            if response.is_ok() {
                self.active_document_key.replace(Some(document_key));
            }
            response
        }

        async fn checkpoint_recovery(
            &self,
            request_id: String,
            document_id: u64,
            base_revision: u64,
            target_name: String,
        ) -> EditorResponse {
            let prepared = self.core.borrow_mut().execute(EditorRequest::PrepareSave {
                request_id,
                document_id,
                base_revision,
                target_name: target_name.clone(),
            })?;
            let EditorReply::SavePrepared {
                save_token,
                file_name,
                bytes,
            } = prepared
            else {
                return Err(worker_error(
                    "unexpected_reply",
                    "core did not prepare recovery bytes",
                ));
            };
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
            Ok(EditorReply::Empty)
        }

        async fn open_local(&self, request_id: String, document_key: String) -> EditorResponse {
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
                    bytes,
                }
            } else {
                EditorRequest::OpenDocument {
                    request_id,
                    file_name: stored.name,
                    bytes,
                }
            };
            let response = self.core.borrow_mut().execute(request);
            if response.is_ok() {
                self.active_document_key.replace(Some(document_key));
            }
            response
        }
    }

    async fn database() -> Result<Rexie, AppErrorDto> {
        Rexie::builder(DATABASE_NAME)
            .version(1)
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
        let value = serde_wasm_bindgen::to_value(document).map_err(js_serde_error)?;
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
        store
            .get(JsValue::from_str(document_key))
            .await
            .map_err(indexed_db_error)?
            .map(|value| serde_wasm_bindgen::from_value(value).map_err(js_serde_error))
            .transpose()
    }

    async fn list_documents() -> EditorResponse {
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
        documents.sort_by_key(|document| std::cmp::Reverse(document.updated_at_ms));
        Ok(EditorReply::LocalDocuments {
            documents: documents
                .into_iter()
                .map(|document| LocalDocumentSummary {
                    id: document.id,
                    name: document.name,
                    updated_at_ms: document.updated_at_ms,
                    has_recovery: document.recovery_bytes.is_some(),
                })
                .collect(),
        })
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

fn main() {}
