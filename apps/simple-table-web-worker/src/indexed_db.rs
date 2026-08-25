use futures::future::LocalBoxFuture;
use rexie::{ObjectStore, Rexie, TransactionMode};
use simple_table_protocol::AppErrorDto;
use std::ops::Deref;
use wasm_bindgen::JsValue;

use crate::workspace::{
    DocumentStore, STORED_DOCUMENT_SCHEMA_VERSION, StoredDocument, worker_error,
};

const DATABASE_NAME: &str = "simple-table";
const DOCUMENTS_STORE: &str = "documents";
const SETTINGS_STORE: &str = "settings";

pub(crate) struct IndexedDbDocumentStore {
    database_name: String,
}

struct DatabaseConnection(Option<Rexie>);

impl DatabaseConnection {
    fn new(database: Rexie) -> Self {
        Self(Some(database))
    }
}

impl Deref for DatabaseConnection {
    type Target = Rexie;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref().expect("open IndexedDB connection")
    }
}

impl Drop for DatabaseConnection {
    fn drop(&mut self) {
        if let Some(database) = self.0.take() {
            database.close();
        }
    }
}

impl IndexedDbDocumentStore {
    pub(crate) fn production() -> Self {
        Self::new(DATABASE_NAME)
    }

    pub(crate) fn new(database_name: impl Into<String>) -> Self {
        Self {
            database_name: database_name.into(),
        }
    }

    async fn database(&self) -> Result<DatabaseConnection, AppErrorDto> {
        Rexie::builder(&self.database_name)
            .version(2)
            .add_object_store(ObjectStore::new(DOCUMENTS_STORE).key_path("id"))
            .add_object_store(ObjectStore::new(SETTINGS_STORE).key_path("key"))
            .build()
            .await
            .map(DatabaseConnection::new)
            .map_err(indexed_db_error)
    }
}

impl DocumentStore for IndexedDbDocumentStore {
    fn put<'a>(
        &'a self,
        document: &'a StoredDocument,
    ) -> LocalBoxFuture<'a, Result<(), AppErrorDto>> {
        Box::pin(async move {
            let database = self.database().await?;
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
        })
    }

    fn get<'a>(
        &'a self,
        document_key: &'a str,
    ) -> LocalBoxFuture<'a, Result<Option<StoredDocument>, AppErrorDto>> {
        Box::pin(async move {
            let database = self.database().await?;
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
                // Migration is best effort so a readable workbook is never blocked by quota pressure.
                let _ = self.put(document).await;
            }
            Ok(document)
        })
    }

    fn list(&self) -> LocalBoxFuture<'_, Result<Vec<StoredDocument>, AppErrorDto>> {
        Box::pin(async move {
            let database = self.database().await?;
            let transaction = database
                .transaction(&[DOCUMENTS_STORE], TransactionMode::ReadOnly)
                .map_err(indexed_db_error)?;
            let store = transaction
                .store(DOCUMENTS_STORE)
                .map_err(indexed_db_error)?;
            let documents = store
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
                let _ = self.put(document).await;
            }
            Ok(documents)
        })
    }

    fn delete<'a>(&'a self, document_key: &'a str) -> LocalBoxFuture<'a, Result<(), AppErrorDto>> {
        Box::pin(async move {
            let database = self.database().await?;
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
        })
    }
}

fn indexed_db_error(error: impl std::fmt::Display) -> AppErrorDto {
    worker_error("indexed_db_error", &error.to_string())
}

fn js_serde_error(error: impl std::fmt::Display) -> AppErrorDto {
    worker_error("indexed_db_serialization_error", &error.to_string())
}

#[cfg(test)]
mod tests {
    use simple_table_protocol::{EditorReply, EditorRequest};
    use simple_table_web_protocol::{
        WebWorkspaceReply, WebWorkspaceRequest, WorkerReply, WorkerRequest,
    };
    use wasm_bindgen_test::*;

    use super::*;
    use crate::workspace::WorkspaceService;

    wasm_bindgen_test_configure!(run_in_browser);

    fn database_name(label: &str) -> String {
        format!("simple-table-test-{label}-{}", uuid::Uuid::new_v4())
    }

    fn stored_document(id: &str) -> StoredDocument {
        StoredDocument {
            schema_version: STORED_DOCUMENT_SCHEMA_VERSION,
            id: id.to_string(),
            name: "browser.xlsx".to_string(),
            saved_bytes: vec![1, 2, 3],
            recovery_bytes: Some(vec![4, 5, 6]),
            saved_content_hash: "hash".to_string(),
            thumbnail: None,
            updated_at_ms: 42,
        }
    }

    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct LegacyStoredDocument {
        id: String,
        name: String,
        #[serde(with = "serde_bytes")]
        saved_bytes: Vec<u8>,
        #[serde(with = "serde_bytes")]
        recovery_bytes: Vec<u8>,
        saved_content_hash: String,
        thumbnail: Option<Vec<u8>>,
        updated_at_ms: u64,
    }

    #[wasm_bindgen_test]
    async fn indexed_db_store_runs_crud_and_lazy_schema_upgrade() {
        let name = database_name("crud");
        Rexie::delete(&name)
            .await
            .expect("delete stale test database");
        let legacy_database = Rexie::builder(&name)
            .version(1)
            .add_object_store(ObjectStore::new(DOCUMENTS_STORE).key_path("id"))
            .build()
            .await
            .expect("create legacy database");
        let transaction = legacy_database
            .transaction(&[DOCUMENTS_STORE], TransactionMode::ReadWrite)
            .expect("legacy transaction");
        let object_store = transaction
            .store(DOCUMENTS_STORE)
            .expect("legacy document store");
        let legacy = serde_wasm_bindgen::to_value(&LegacyStoredDocument {
            id: "legacy-document".to_string(),
            name: "legacy.xlsx".to_string(),
            saved_bytes: vec![9, 8, 7],
            recovery_bytes: vec![6, 5],
            saved_content_hash: "legacy-hash".to_string(),
            thumbnail: None,
            updated_at_ms: 21,
        })
        .expect("serialize legacy record");
        object_store
            .put(&legacy, None)
            .await
            .expect("put legacy record");
        transaction.done().await.expect("commit legacy record");
        legacy_database.close();

        let store = IndexedDbDocumentStore::new(&name);
        let legacy = store
            .get("legacy-document")
            .await
            .expect("get legacy document")
            .expect("legacy document");
        assert_eq!(legacy.schema_version, 1);
        assert_eq!(legacy.saved_bytes, vec![9, 8, 7]);
        assert_eq!(legacy.recovery_bytes, Some(vec![6, 5]));
        assert_eq!(
            store
                .get("legacy-document")
                .await
                .expect("get upgraded document")
                .expect("upgraded document")
                .schema_version,
            STORED_DOCUMENT_SCHEMA_VERSION
        );

        let document = stored_document("document-1");

        store.put(&document).await.expect("put document");
        let loaded = store
            .get("document-1")
            .await
            .expect("get document")
            .expect("stored document");
        assert_eq!(loaded.saved_bytes, vec![1, 2, 3]);
        assert_eq!(loaded.recovery_bytes, Some(vec![4, 5, 6]));
        assert_eq!(store.list().await.expect("list documents").len(), 2);
        store.delete("document-1").await.expect("delete document");
        store
            .delete("legacy-document")
            .await
            .expect("delete legacy document");
        assert!(
            store
                .get("document-1")
                .await
                .expect("get deleted")
                .is_none()
        );

        Rexie::delete(&name).await.expect("delete test database");
    }

    #[wasm_bindgen_test]
    async fn indexed_db_workspace_round_trips_recovery_save_and_delete() {
        let name = database_name("workspace");
        Rexie::delete(&name)
            .await
            .expect("delete stale test database");
        let source = WorkspaceService::new(IndexedDbDocumentStore::new(&name));
        let (reply, _) = source
            .execute(
                WorkerRequest::Editor(EditorRequest::NewDocument {
                    request_id: "new".to_string(),
                }),
                None,
            )
            .await;
        let Ok(WorkerReply::Editor(EditorReply::Document {
            value: Some(document),
        })) = reply
        else {
            panic!("expected new document")
        };
        let (reply, _) = source
            .execute(
                WorkerRequest::Workspace(WebWorkspaceRequest::CheckpointRecovery {
                    request_id: "checkpoint".to_string(),
                    document_id: document.editor_session.document_id,
                    base_revision: document.editor_session.revision,
                    target_name: "browser.xlsx".to_string(),
                }),
                None,
            )
            .await;
        assert!(matches!(
            reply,
            Ok(WorkerReply::Workspace(WebWorkspaceReply::Empty))
        ));
        let summaries = match source
            .execute(
                WorkerRequest::Workspace(WebWorkspaceRequest::ListLocalDocuments),
                None,
            )
            .await
            .0
            .expect("list response")
        {
            WorkerReply::Workspace(WebWorkspaceReply::LocalDocuments(documents)) => documents,
            _ => panic!("expected local documents"),
        };
        assert_eq!(summaries.len(), 1);
        assert!(summaries[0].has_recovery);
        let document_key = summaries[0].id.clone();

        let recovered = WorkspaceService::new(IndexedDbDocumentStore::new(&name));
        let (reply, _) = recovered
            .execute(
                WorkerRequest::Workspace(WebWorkspaceRequest::OpenLocalDocument {
                    request_id: "open".to_string(),
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
                    request_id: "save".to_string(),
                    document_id: document.editor_session.document_id,
                    base_revision: document.editor_session.revision,
                    target_name: "browser.xlsx".to_string(),
                }),
                None,
            )
            .await;
        assert!(matches!(
            reply,
            Ok(WorkerReply::Workspace(WebWorkspaceReply::Saved(_)))
        ));
        let stored = IndexedDbDocumentStore::new(&name)
            .get(&document_key)
            .await
            .expect("get saved document")
            .expect("saved document");
        assert!(stored.recovery_bytes.is_none());

        let (reply, _) = recovered
            .execute(
                WorkerRequest::Workspace(WebWorkspaceRequest::DeleteLocalDocument { document_key }),
                None,
            )
            .await;
        assert!(matches!(
            reply,
            Ok(WorkerReply::Workspace(WebWorkspaceReply::Empty))
        ));
        Rexie::delete(&name).await.expect("delete test database");
    }
}
