use std::cell::RefCell;

use futures::future::LocalBoxFuture;
use sha2::{Digest, Sha256};
use simple_table_engine::CoreFacade;
use simple_table_protocol::{AppErrorDto, EditorCommand, EditorOutput, EditorReply, EditorRequest};
use simple_table_web_protocol::{
    LocalDocumentSummary, WebWorkspaceReply, WebWorkspaceRequest, WorkerReply, WorkerRequest,
};

pub(crate) const STORED_DOCUMENT_SCHEMA_VERSION: u16 = 2;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredDocument {
    #[serde(default = "legacy_schema_version")]
    pub(crate) schema_version: u16,
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(with = "serde_bytes")]
    pub(crate) saved_bytes: Vec<u8>,
    #[serde(default, with = "option_bytes")]
    pub(crate) recovery_bytes: Option<Vec<u8>>,
    pub(crate) saved_content_hash: String,
    #[serde(default, with = "option_bytes")]
    pub(crate) thumbnail: Option<Vec<u8>>,
    pub(crate) updated_at_ms: u64,
}

fn legacy_schema_version() -> u16 {
    1
}

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

pub(crate) trait DocumentStore {
    fn put<'a>(
        &'a self,
        document: &'a StoredDocument,
    ) -> LocalBoxFuture<'a, Result<(), AppErrorDto>>;
    fn get<'a>(
        &'a self,
        document_key: &'a str,
    ) -> LocalBoxFuture<'a, Result<Option<StoredDocument>, AppErrorDto>>;
    fn list(&self) -> LocalBoxFuture<'_, Result<Vec<StoredDocument>, AppErrorDto>>;
    fn delete<'a>(&'a self, document_key: &'a str) -> LocalBoxFuture<'a, Result<(), AppErrorDto>>;
}

pub(crate) struct WorkspaceService<S> {
    core: RefCell<CoreFacade>,
    active_document_key: RefCell<Option<String>>,
    store: S,
}

impl<S: DocumentStore> WorkspaceService<S> {
    pub(crate) fn new(store: S) -> Self {
        Self {
            core: RefCell::new(CoreFacade::default()),
            active_document_key: RefCell::new(None),
            store,
        }
    }

    pub(crate) async fn execute(
        &self,
        request: WorkerRequest,
        attachment: Option<Vec<u8>>,
    ) -> (Result<WorkerReply, AppErrorDto>, Option<Vec<u8>>) {
        let result = match request {
            WorkerRequest::Editor(request) => self.execute_editor(request, attachment).await,
            WorkerRequest::Workspace(request) => {
                if attachment.is_some() {
                    Err(worker_error(
                        "worker_protocol_error",
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
            self.clear_recovery(document_key).await?;
        }
        let EditorOutput { reply, attachment } = self.core.borrow_mut().execute(EditorCommand {
            request,
            attachment,
        })?;
        if replaces_document {
            if let Some(document_key) = previous_key {
                // Failure keeps the old recovery record available instead of failing the new open.
                let _ = self.clear_recovery(&document_key).await;
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
                    && self.clear_recovery(&document_key).await?
                {
                    self.active_document_key.replace(None);
                }
                Ok((WorkerReply::Workspace(WebWorkspaceReply::Empty), None))
            }
            WebWorkspaceRequest::ListLocalDocuments => {
                let mut documents = self.store.list().await?;
                documents.sort_by_key(|document| std::cmp::Reverse(document.updated_at_ms));
                Ok((
                    WorkerReply::Workspace(WebWorkspaceReply::LocalDocuments(
                        documents
                            .into_iter()
                            .map(|document| LocalDocumentSummary {
                                id: document.id,
                                name: document.name,
                                updated_at_ms: document.updated_at_ms,
                                has_recovery: document.recovery_bytes.is_some(),
                            })
                            .collect(),
                    )),
                    None,
                ))
            }
            WebWorkspaceRequest::OpenLocalDocument {
                request_id,
                document_key,
            } => self.open_local(request_id, document_key).await,
            WebWorkspaceRequest::DeleteLocalDocument { document_key } => {
                self.store.delete(&document_key).await?;
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
                "core did not prepare a save",
            ));
        };
        let bytes = attachment
            .ok_or_else(|| worker_error("unexpected_reply", "core omitted prepared save bytes"))?;
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
        if let Err(error) = self.store.put(&stored).await {
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
            target_name,
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
        let mut stored = self
            .store
            .get(&document_key)
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
        self.store.put(&stored).await?;
        self.active_document_key.replace(Some(document_key));
        Ok((WorkerReply::Workspace(WebWorkspaceReply::Empty), None))
    }

    async fn open_local(
        &self,
        request_id: String,
        document_key: String,
    ) -> Result<(WorkerReply, Option<Vec<u8>>), AppErrorDto> {
        let stored = self
            .store
            .get(&document_key)
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

    async fn clear_recovery(&self, document_key: &str) -> Result<bool, AppErrorDto> {
        let Some(mut document) = self.store.get(document_key).await? else {
            return Ok(true);
        };
        if document.saved_bytes.is_empty() {
            self.store.delete(document_key).await?;
            return Ok(true);
        }
        if document.recovery_bytes.take().is_some() {
            self.store.put(&document).await?;
        }
        Ok(false)
    }
}

fn content_hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn now_ms() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now().max(0.0) as u64
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
    }
}

pub(crate) fn worker_error(code: &str, message: &str) -> AppErrorDto {
    AppErrorDto {
        code: code.to_string(),
        message: message.to_string(),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::collections::{HashMap, VecDeque};
    use std::rc::Rc;

    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum StoreOperation {
        Get,
        Put,
        List,
        Delete,
    }

    #[derive(Clone, Default)]
    struct MemoryDocumentStore {
        documents: Rc<RefCell<HashMap<String, StoredDocument>>>,
        failures: Rc<RefCell<VecDeque<StoreOperation>>>,
    }

    impl MemoryDocumentStore {
        fn fail_next(&self, operation: StoreOperation) {
            self.failures.borrow_mut().push_back(operation);
        }

        fn failure(&self, operation: StoreOperation) -> Result<(), AppErrorDto> {
            if self.failures.borrow().front() == Some(&operation) {
                self.failures.borrow_mut().pop_front();
                Err(worker_error("memory_store_error", "injected store failure"))
            } else {
                Ok(())
            }
        }
    }

    impl DocumentStore for MemoryDocumentStore {
        fn put<'a>(
            &'a self,
            document: &'a StoredDocument,
        ) -> LocalBoxFuture<'a, Result<(), AppErrorDto>> {
            Box::pin(async move {
                self.failure(StoreOperation::Put)?;
                let mut document = document.clone();
                document.schema_version = STORED_DOCUMENT_SCHEMA_VERSION;
                self.documents
                    .borrow_mut()
                    .insert(document.id.clone(), document);
                Ok(())
            })
        }

        fn get<'a>(
            &'a self,
            document_key: &'a str,
        ) -> LocalBoxFuture<'a, Result<Option<StoredDocument>, AppErrorDto>> {
            Box::pin(async move {
                self.failure(StoreOperation::Get)?;
                Ok(self.documents.borrow().get(document_key).cloned())
            })
        }

        fn list(&self) -> LocalBoxFuture<'_, Result<Vec<StoredDocument>, AppErrorDto>> {
            Box::pin(async move {
                self.failure(StoreOperation::List)?;
                Ok(self.documents.borrow().values().cloned().collect())
            })
        }

        fn delete<'a>(
            &'a self,
            document_key: &'a str,
        ) -> LocalBoxFuture<'a, Result<(), AppErrorDto>> {
            Box::pin(async move {
                self.failure(StoreOperation::Delete)?;
                self.documents.borrow_mut().remove(document_key);
                Ok(())
            })
        }
    }

    async fn new_document(service: &WorkspaceService<MemoryDocumentStore>) -> (u64, u64) {
        let (reply, attachment) = service
            .execute(
                WorkerRequest::Editor(EditorRequest::NewDocument {
                    request_id: "new".to_string(),
                }),
                None,
            )
            .await;
        assert!(attachment.is_none());
        let WorkerReply::Editor(EditorReply::Document { value: Some(value) }) = reply.unwrap()
        else {
            panic!("expected new document")
        };
        (
            value.editor_session.document_id,
            value.editor_session.revision,
        )
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
    }

    #[test]
    fn save_failure_aborts_the_prepared_save_and_keeps_the_document_dirty() {
        futures::executor::block_on(async {
            let store = MemoryDocumentStore::default();
            let service = WorkspaceService::new(store.clone());
            let (document_id, revision) = new_document(&service).await;
            store.fail_next(StoreOperation::Put);

            let (reply, _) = service
                .execute(
                    WorkerRequest::Workspace(WebWorkspaceRequest::SaveLocal {
                        request_id: "save".to_string(),
                        document_id,
                        base_revision: revision,
                        target_name: "book.xlsx".to_string(),
                    }),
                    None,
                )
                .await;

            assert_eq!(reply.unwrap_err().code, "memory_store_error");
            let (reply, _) = service
                .execute(WorkerRequest::Editor(EditorRequest::ActiveDocument), None)
                .await;
            let WorkerReply::Editor(EditorReply::Document { value: Some(value) }) = reply.unwrap()
            else {
                panic!("expected active document")
            };
            assert!(value.editor_session.editor_state.is_dirty);
            assert!(store.documents.borrow().is_empty());
        });
    }

    #[test]
    fn recovery_round_trip_save_and_delete_use_the_same_store_port() {
        futures::executor::block_on(async {
            let store = MemoryDocumentStore::default();
            let source = WorkspaceService::new(store.clone());
            let (document_id, revision) = new_document(&source).await;
            let (reply, _) = source
                .execute(
                    WorkerRequest::Workspace(WebWorkspaceRequest::CheckpointRecovery {
                        request_id: "checkpoint".to_string(),
                        document_id,
                        base_revision: revision,
                        target_name: "recovered.xlsx".to_string(),
                    }),
                    None,
                )
                .await;
            assert!(matches!(
                reply,
                Ok(WorkerReply::Workspace(WebWorkspaceReply::Empty))
            ));
            let document_key = store
                .documents
                .borrow()
                .keys()
                .next()
                .cloned()
                .expect("recovery record");

            let recovered = WorkspaceService::new(store.clone());
            let (reply, _) = recovered
                .execute(
                    WorkerRequest::Workspace(WebWorkspaceRequest::OpenLocalDocument {
                        request_id: "open-recovery".to_string(),
                        document_key: document_key.clone(),
                    }),
                    None,
                )
                .await;
            let Ok(WorkerReply::Workspace(WebWorkspaceReply::Document(document))) = reply else {
                panic!("expected recovered document")
            };
            assert!(document.editor_session.editor_state.is_dirty);

            let (reply, _) = recovered
                .execute(
                    WorkerRequest::Workspace(WebWorkspaceRequest::SaveLocal {
                        request_id: "save-recovery".to_string(),
                        document_id: document.editor_session.document_id,
                        base_revision: document.editor_session.revision,
                        target_name: "recovered.xlsx".to_string(),
                    }),
                    None,
                )
                .await;
            assert!(matches!(
                reply,
                Ok(WorkerReply::Workspace(WebWorkspaceReply::Saved(_)))
            ));
            assert!(
                store
                    .documents
                    .borrow()
                    .get(&document_key)
                    .is_some_and(|document| document.recovery_bytes.is_none())
            );

            let (reply, _) = recovered
                .execute(
                    WorkerRequest::Workspace(WebWorkspaceRequest::DeleteLocalDocument {
                        document_key,
                    }),
                    None,
                )
                .await;
            assert!(matches!(
                reply,
                Ok(WorkerReply::Workspace(WebWorkspaceReply::Empty))
            ));
            assert!(store.documents.borrow().is_empty());
        });
    }

    #[test]
    fn get_list_and_delete_failures_are_propagated() {
        futures::executor::block_on(async {
            let store = MemoryDocumentStore::default();
            let service = WorkspaceService::new(store.clone());

            store.fail_next(StoreOperation::Get);
            let (reply, _) = service
                .execute(
                    WorkerRequest::Workspace(WebWorkspaceRequest::OpenLocalDocument {
                        request_id: "open".to_string(),
                        document_key: "missing".to_string(),
                    }),
                    None,
                )
                .await;
            assert_eq!(reply.unwrap_err().code, "memory_store_error");

            store.fail_next(StoreOperation::List);
            let (reply, _) = service
                .execute(
                    WorkerRequest::Workspace(WebWorkspaceRequest::ListLocalDocuments),
                    None,
                )
                .await;
            assert_eq!(reply.unwrap_err().code, "memory_store_error");

            store.fail_next(StoreOperation::Delete);
            let (reply, _) = service
                .execute(
                    WorkerRequest::Workspace(WebWorkspaceRequest::DeleteLocalDocument {
                        document_key: "missing".to_string(),
                    }),
                    None,
                )
                .await;
            assert_eq!(reply.unwrap_err().code, "memory_store_error");
        });
    }
}
