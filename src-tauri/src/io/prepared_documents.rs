use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use crate::error::AppError;
use crate::state::editor_state::EditorState;

pub(crate) struct PreparedDocument {
    pub(crate) editor_state: EditorState,
    pub(crate) source_path: Option<PathBuf>,
}

struct PreparedDocumentStore {
    pending: HashMap<String, PreparedDocument>,
    order: VecDeque<String>,
}

impl Default for PreparedDocumentStore {
    fn default() -> Self {
        Self {
            pending: HashMap::new(),
            order: VecDeque::new(),
        }
    }
}

impl PreparedDocumentStore {
    fn insert(&mut self, token: String, prepared: PreparedDocument) {
        while self.pending.len() >= MAX_PREPARED_DOCUMENTS {
            let Some(expired) = self.order.pop_front() else {
                break;
            };
            self.pending.remove(&expired);
        }
        self.order.push_back(token.clone());
        self.pending.insert(token, prepared);
    }

    fn take(&mut self, token: &str) -> Option<PreparedDocument> {
        let prepared = self.pending.remove(token)?;
        self.order.retain(|pending_token| pending_token != token);
        Some(prepared)
    }

    fn abort(&mut self, token: &str) {
        self.pending.remove(token);
        self.order.retain(|pending_token| pending_token != token);
    }
}

const MAX_PREPARED_DOCUMENTS: usize = 8;

static PREPARED_DOCUMENTS: OnceLock<Mutex<PreparedDocumentStore>> = OnceLock::new();

fn store() -> &'static Mutex<PreparedDocumentStore> {
    PREPARED_DOCUMENTS.get_or_init(|| Mutex::new(PreparedDocumentStore::default()))
}

pub(crate) fn replace(
    editor_state: EditorState,
    source_path: Option<PathBuf>,
) -> Result<String, AppError> {
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
}
