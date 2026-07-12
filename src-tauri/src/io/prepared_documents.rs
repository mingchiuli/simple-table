use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::error::AppError;
use crate::state::editor_state::EditorState;

pub(crate) struct PreparedDocument {
    pub(crate) editor_state: EditorState,
    pub(crate) source_path: Option<PathBuf>,
}

#[derive(Default)]
struct PreparedDocumentStore {
    pending: HashMap<String, PreparedDocumentEntry>,
    order: VecDeque<String>,
}

struct PreparedDocumentEntry {
    document: PreparedDocument,
    created_at: Instant,
}

impl PreparedDocumentStore {
    fn insert(&mut self, token: String, prepared: PreparedDocument) {
        self.insert_at(token, prepared, Instant::now());
    }

    fn insert_at(&mut self, token: String, prepared: PreparedDocument, now: Instant) {
        self.prune_expired(now);
        while self.pending.len() >= MAX_PREPARED_DOCUMENTS {
            let Some(expired) = self.order.pop_front() else {
                break;
            };
            self.pending.remove(&expired);
        }
        self.order.push_back(token.clone());
        self.pending.insert(
            token,
            PreparedDocumentEntry {
                document: prepared,
                created_at: now,
            },
        );
    }

    fn take(&mut self, token: &str) -> Option<PreparedDocument> {
        self.prune_expired(Instant::now());
        let prepared = self.pending.remove(token)?.document;
        self.order.retain(|pending_token| pending_token != token);
        Some(prepared)
    }

    fn abort(&mut self, token: &str) {
        self.prune_expired(Instant::now());
        self.pending.remove(token);
        self.order.retain(|pending_token| pending_token != token);
    }

    fn prune_expired(&mut self, now: Instant) {
        self.pending.retain(|_, entry| {
            now.saturating_duration_since(entry.created_at) < PREPARED_DOCUMENT_TTL
        });
        self.order
            .retain(|pending_token| self.pending.contains_key(pending_token));
    }
}

const MAX_PREPARED_DOCUMENTS: usize = 1;
const MAX_PREPARED_DOCUMENT_BYTES: usize = 128 * 1024 * 1024;
const PREPARED_DOCUMENT_TTL: Duration = Duration::from_secs(5 * 60);

static PREPARED_DOCUMENTS: OnceLock<Mutex<PreparedDocumentStore>> = OnceLock::new();

fn store() -> &'static Mutex<PreparedDocumentStore> {
    PREPARED_DOCUMENTS.get_or_init(|| Mutex::new(PreparedDocumentStore::default()))
}

pub(crate) fn replace(
    editor_state: EditorState,
    source_path: Option<PathBuf>,
) -> Result<String, AppError> {
    let estimated_bytes = editor_state.estimated_resource_bytes();
    if estimated_bytes > MAX_PREPARED_DOCUMENT_BYTES {
        return Err(AppError::ResourceLimitExceeded(format!(
            "prepared document requires an estimated {estimated_bytes} bytes, maximum is {MAX_PREPARED_DOCUMENT_BYTES}"
        )));
    }
    let token = uuid::Uuid::new_v4().to_string();
    let prepared = PreparedDocument {
        editor_state,
        source_path,
    };
    let mut store = store()
        .lock()
        .map_err(|_| AppError::poisoned_lock("prepared document store"))?;
    store.insert(token.clone(), prepared);
    Ok(token)
}

pub(crate) fn take(token: &str) -> Result<PreparedDocument, AppError> {
    let mut store = store()
        .lock()
        .map_err(|_| AppError::poisoned_lock("prepared document store"))?;
    store.take(token).ok_or_else(|| {
        AppError::DocumentStateInvalid("prepared document token is no longer active".to_string())
    })
}

pub(crate) fn abort(token: &str) -> Result<(), AppError> {
    let mut store = store()
        .lock()
        .map_err(|_| AppError::poisoned_lock("prepared document store"))?;
    store.abort(token);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FileData, SheetData};

    fn prepared(name: &str) -> PreparedDocument {
        PreparedDocument {
            editor_state: EditorState::with_workbook(
                FileData {
                    path: String::new(),
                    file_name: name.to_string(),
                    sheets: vec![SheetData::default()],
                },
                None,
            ),
            source_path: None,
        }
    }

    #[test]
    fn taking_a_token_consumes_it() {
        let mut store = PreparedDocumentStore::default();
        store.insert("token".to_string(), prepared("book.xlsx"));

        let document = store.take("token").expect("prepared document");

        assert_eq!(document.editor_state.file_data().file_name, "book.xlsx");
        assert!(store.take("token").is_none());
        assert!(store.order.is_empty());
    }

    #[test]
    fn abort_is_idempotent() {
        let mut store = PreparedDocumentStore::default();
        store.insert("token".to_string(), prepared("book.xlsx"));

        store.abort("token");
        store.abort("token");

        assert!(store.take("token").is_none());
        assert!(store.order.is_empty());
    }

    #[test]
    fn insertion_evicts_the_oldest_token_at_the_limit() {
        let mut store = PreparedDocumentStore::default();
        for index in 0..=MAX_PREPARED_DOCUMENTS {
            store.insert(
                format!("token-{index}"),
                prepared(&format!("book-{index}.xlsx")),
            );
        }

        assert!(store.take("token-0").is_none());
        assert!(store.take("token-1").is_some());
        assert_eq!(store.pending.len(), MAX_PREPARED_DOCUMENTS - 1);
    }

    #[test]
    fn insertion_prunes_expired_documents_before_enforcing_capacity() {
        let mut store = PreparedDocumentStore::default();
        let now = Instant::now();
        store.insert_at(
            "expired".to_string(),
            prepared("expired.xlsx"),
            now - PREPARED_DOCUMENT_TTL,
        );

        store.insert_at("current".to_string(), prepared("current.xlsx"), now);

        assert!(!store.pending.contains_key("expired"));
        assert!(store.pending.contains_key("current"));
        assert_eq!(store.order, VecDeque::from(["current".to_string()]));
    }

    #[test]
    fn taking_an_expired_token_removes_it() {
        let mut store = PreparedDocumentStore::default();
        store.insert_at(
            "expired".to_string(),
            prepared("expired.xlsx"),
            Instant::now() - PREPARED_DOCUMENT_TTL,
        );

        assert!(store.take("expired").is_none());
        assert!(store.pending.is_empty());
        assert!(store.order.is_empty());
    }
}
