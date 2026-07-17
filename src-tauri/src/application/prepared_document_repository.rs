use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::error::AppError;
use crate::resource_limits::ResourceLedger;
use crate::state::editor_state::EditorState;
use crate::types::FileData;

pub(crate) struct PreparedDocument {
    pub(crate) editor_state: EditorState,
    pub(crate) source_path: Option<PathBuf>,
}

#[derive(Default)]
struct PreparedDocumentStore {
    pending: HashMap<String, PreparedDocumentEntry>,
    order: VecDeque<String>,
    retired: Vec<PreparedDocument>,
    prepare_in_progress: bool,
    checkout_in_progress: bool,
}

struct PreparedDocumentEntry {
    document: PreparedDocument,
    created_at: Instant,
}

impl PreparedDocumentStore {
    fn begin_prepare(&mut self, now: Instant) -> Result<(), AppError> {
        self.prune_expired(now);
        if self.prepare_in_progress
            || self.checkout_in_progress
            || self.pending.len() >= MAX_PREPARED_DOCUMENTS
        {
            return Err(AppError::PreparedDocumentConflict);
        }
        self.prepare_in_progress = true;
        Ok(())
    }

    fn finish_prepare(&mut self) {
        self.prepare_in_progress = false;
    }

    fn insert(&mut self, token: String, prepared: PreparedDocument) -> Result<(), AppError> {
        self.insert_at(token, prepared, Instant::now())
    }

    fn insert_at(
        &mut self,
        token: String,
        prepared: PreparedDocument,
        now: Instant,
    ) -> Result<(), AppError> {
        self.prune_expired(now);
        if self.pending.len() >= MAX_PREPARED_DOCUMENTS {
            self.retired.push(prepared);
            return Err(AppError::PreparedDocumentConflict);
        }
        self.order.push_back(token.clone());
        if let Some(previous) = self.pending.insert(
            token,
            PreparedDocumentEntry {
                document: prepared,
                created_at: now,
            },
        ) {
            self.retired.push(previous.document);
        }
        Ok(())
    }

    #[cfg(test)]
    fn take(&mut self, token: &str) -> Option<PreparedDocument> {
        self.prune_expired(Instant::now());
        let prepared = self.pending.remove(token)?.document;
        self.order.retain(|pending_token| pending_token != token);
        Some(prepared)
    }

    fn checkout(&mut self, token: &str) -> Option<PreparedDocumentEntry> {
        self.prune_expired(Instant::now());
        if self.checkout_in_progress {
            return None;
        }
        let entry = self.pending.remove(token)?;
        self.order.retain(|pending_token| pending_token != token);
        self.checkout_in_progress = true;
        Some(entry)
    }

    fn restore_checkout(&mut self, token: String, entry: PreparedDocumentEntry) {
        self.checkout_in_progress = false;
        self.order.push_back(token.clone());
        if let Some(previous) = self.pending.insert(token, entry) {
            self.retired.push(previous.document);
        }
    }

    fn finish_checkout(&mut self) {
        self.checkout_in_progress = false;
    }

    fn abort(&mut self, token: &str) {
        self.prune_expired(Instant::now());
        if let Some(entry) = self.pending.remove(token) {
            self.retired.push(entry.document);
        }
        self.order.retain(|pending_token| pending_token != token);
    }

    fn prune_expired(&mut self, now: Instant) {
        let expired: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, entry)| {
                now.saturating_duration_since(entry.created_at) >= PREPARED_DOCUMENT_TTL
            })
            .map(|(token, _)| token.clone())
            .collect();
        for token in expired {
            if let Some(entry) = self.pending.remove(&token) {
                self.retired.push(entry.document);
            }
        }
        self.order
            .retain(|pending_token| self.pending.contains_key(pending_token));
    }

    fn take_retired(&mut self) -> Vec<PreparedDocument> {
        std::mem::take(&mut self.retired)
    }
}

const MAX_PREPARED_DOCUMENTS: usize = 1;
const MAX_PREPARED_DOCUMENT_BYTES: usize = 128 * 1024 * 1024;
const MAX_ACTIVE_AND_PREPARED_DOCUMENT_BYTES: usize = 256 * 1024 * 1024;
const PREPARED_DOCUMENT_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Default)]
pub(crate) struct PreparedDocumentRepository {
    store: Arc<Mutex<PreparedDocumentStore>>,
}

pub(crate) struct PrepareReservation {
    repository: PreparedDocumentRepository,
    active: bool,
}

pub(crate) struct PreparedDocumentCheckout {
    repository: PreparedDocumentRepository,
    token: String,
    entry: Option<PreparedDocumentEntry>,
}

pub(crate) struct PreparedDocumentCommit {
    repository: PreparedDocumentRepository,
    active: bool,
}

impl PreparedDocumentCheckout {
    pub(crate) fn document(&self) -> &PreparedDocument {
        &self.entry.as_ref().expect("checkout entry").document
    }

    pub(crate) fn commit(mut self) -> (PreparedDocument, PreparedDocumentCommit) {
        (
            self.entry.take().expect("checkout entry").document,
            PreparedDocumentCommit {
                repository: self.repository.clone(),
                active: true,
            },
        )
    }
}

impl Drop for PreparedDocumentCheckout {
    fn drop(&mut self) {
        let Some(entry) = self.entry.take() else {
            return;
        };
        let retired = match self.repository.store.lock() {
            Ok(mut store) => {
                store.restore_checkout(self.token.clone(), entry);
                store.take_retired()
            }
            Err(_) => {
                drop(entry);
                return;
            }
        };
        drop(retired);
    }
}

impl Drop for PreparedDocumentCommit {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        if let Ok(mut store) = self.repository.store.lock() {
            store.finish_checkout();
            self.active = false;
        }
    }
}

impl Drop for PrepareReservation {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        if let Ok(mut store) = self.repository.store.lock() {
            store.finish_prepare();
        }
    }
}

impl PreparedDocumentRepository {
    #[cfg(test)]
    pub(crate) fn is_same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.store, &other.store)
    }

    pub(crate) fn reserve_for_parse_bytes(
        &self,
        estimated_parse_bytes: usize,
        active_document_bytes: usize,
    ) -> Result<PrepareReservation, AppError> {
        validate_prepared_document_bytes(estimated_parse_bytes)?;
        self.reserve_prepare(estimated_parse_bytes, active_document_bytes)
    }

    pub(crate) fn reserve_for_file_data(
        &self,
        file_data: &FileData,
        active_document_bytes: usize,
    ) -> Result<PrepareReservation, AppError> {
        let estimated_bytes = ResourceLedger::from_file_data(file_data)
            .estimated_bytes()
            .saturating_mul(2);
        validate_prepared_document_bytes(estimated_bytes)?;
        self.reserve_prepare(estimated_bytes, active_document_bytes)
    }

    fn reserve_prepare(
        &self,
        estimated_bytes: usize,
        active_document_bytes: usize,
    ) -> Result<PrepareReservation, AppError> {
        validate_combined_document_bytes_for_active(active_document_bytes, estimated_bytes)?;
        let (result, retired) = {
            let mut store = self
                .store
                .lock()
                .map_err(|_| AppError::poisoned_lock("prepared document store"))?;
            let result = store.begin_prepare(Instant::now());
            (result, store.take_retired())
        };
        drop(retired);
        result?;
        Ok(PrepareReservation {
            repository: self.clone(),
            active: true,
        })
    }
}

fn validate_prepared_document_bytes(estimated_bytes: usize) -> Result<(), AppError> {
    if estimated_bytes > MAX_PREPARED_DOCUMENT_BYTES {
        return Err(AppError::ResourceLimitExceeded(format!(
            "prepared document parse requires an estimated {estimated_bytes} bytes, maximum is {MAX_PREPARED_DOCUMENT_BYTES}"
        )));
    }
    Ok(())
}

impl PreparedDocumentRepository {
    pub(crate) fn replace(
        &self,
        editor_state: EditorState,
        source_path: Option<PathBuf>,
        mut reservation: PrepareReservation,
        active_document_bytes: usize,
    ) -> Result<String, AppError> {
        let estimated_bytes = editor_state.estimated_resource_bytes();
        validate_prepared_document_bytes(estimated_bytes)?;
        let token = uuid::Uuid::new_v4().to_string();
        let prepared = PreparedDocument {
            editor_state,
            source_path,
        };
        validate_combined_document_bytes_for_active(active_document_bytes, estimated_bytes)?;
        let (result, retired) = {
            let mut store = self
                .store
                .lock()
                .map_err(|_| AppError::poisoned_lock("prepared document store"))?;
            store.finish_prepare();
            reservation.active = false;
            let result = store.insert(token.clone(), prepared);
            (result, store.take_retired())
        };
        drop(retired);
        result?;
        Ok(token)
    }
}

fn validate_combined_document_bytes_for_active(
    active_bytes: usize,
    prepared_bytes: usize,
) -> Result<(), AppError> {
    let peak_bytes = active_bytes.saturating_add(prepared_bytes);
    if peak_bytes > MAX_ACTIVE_AND_PREPARED_DOCUMENT_BYTES {
        return Err(AppError::ResourceLimitExceeded(format!(
            "active and prepared documents require an estimated {peak_bytes} bytes, maximum is {MAX_ACTIVE_AND_PREPARED_DOCUMENT_BYTES}"
        )));
    }
    Ok(())
}

impl PreparedDocumentRepository {
    #[cfg(test)]
    pub(crate) fn take(&self, token: &str) -> Result<PreparedDocument, AppError> {
        let (prepared, retired) = {
            let mut store = self
                .store
                .lock()
                .map_err(|_| AppError::poisoned_lock("prepared document store"))?;
            let prepared = store.take(token);
            (prepared, store.take_retired())
        };
        drop(retired);
        prepared.ok_or_else(|| {
            AppError::DocumentStateInvalid(
                "prepared document token is no longer active".to_string(),
            )
        })
    }

    pub(crate) fn checkout(&self, token: &str) -> Result<PreparedDocumentCheckout, AppError> {
        let (entry, retired) = {
            let mut store = self
                .store
                .lock()
                .map_err(|_| AppError::poisoned_lock("prepared document store"))?;
            let entry = store.checkout(token);
            (entry, store.take_retired())
        };
        drop(retired);
        let entry = entry.ok_or_else(|| {
            AppError::DocumentStateInvalid(
                "prepared document token is no longer active".to_string(),
            )
        })?;
        Ok(PreparedDocumentCheckout {
            repository: self.clone(),
            token: token.to_string(),
            entry: Some(entry),
        })
    }

    pub(crate) fn abort(&self, token: &str) -> Result<(), AppError> {
        let retired = {
            let mut store = self
                .store
                .lock()
                .map_err(|_| AppError::poisoned_lock("prepared document store"))?;
            store.abort(token);
            store.take_retired()
        };
        drop(retired);
        Ok(())
    }
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
        store
            .insert("token".to_string(), prepared("book.xlsx"))
            .expect("insert token");

        let document = store.take("token").expect("prepared document");

        assert_eq!(document.editor_state.file_data().file_name, "book.xlsx");
        assert!(store.take("token").is_none());
        assert!(store.order.is_empty());
    }

    #[test]
    fn dropped_checkout_restores_the_prepared_document() {
        let mut store = PreparedDocumentStore::default();
        store
            .insert("token".to_string(), prepared("book.xlsx"))
            .expect("insert token");
        let entry = store.checkout("token").expect("checkout");
        assert!(store.begin_prepare(Instant::now()).is_err());

        store.restore_checkout("token".to_string(), entry);

        assert!(store.take("token").is_some());
        assert!(!store.checkout_in_progress);
    }

    #[test]
    fn abort_is_idempotent() {
        let mut store = PreparedDocumentStore::default();
        store
            .insert("token".to_string(), prepared("book.xlsx"))
            .expect("insert token");

        store.abort("token");
        store.abort("token");

        assert!(store.take("token").is_none());
        assert!(store.order.is_empty());
        assert_eq!(store.take_retired().len(), 1);
    }

    #[test]
    fn insertion_rejects_a_second_live_token_at_the_limit() {
        let mut store = PreparedDocumentStore::default();
        store
            .insert("token-0".to_string(), prepared("book-0.xlsx"))
            .expect("insert first token");

        assert!(matches!(
            store.insert("token-1".to_string(), prepared("book-1.xlsx")),
            Err(AppError::PreparedDocumentConflict)
        ));
        assert_eq!(store.take_retired().len(), 1);
        assert!(store.take("token-0").is_some());
        assert!(store.take("token-1").is_none());
    }

    #[test]
    fn preparation_reservation_rejects_concurrent_parse_before_insertion() {
        let mut store = PreparedDocumentStore::default();

        store
            .begin_prepare(Instant::now())
            .expect("reserve prepare");

        assert!(matches!(
            store.begin_prepare(Instant::now()),
            Err(AppError::PreparedDocumentConflict)
        ));
        store.finish_prepare();
        assert!(store.begin_prepare(Instant::now()).is_ok());
    }

    #[test]
    fn insertion_prunes_expired_documents_before_enforcing_capacity() {
        let mut store = PreparedDocumentStore::default();
        let now = Instant::now();
        store
            .insert_at(
                "expired".to_string(),
                prepared("expired.xlsx"),
                now - PREPARED_DOCUMENT_TTL,
            )
            .expect("insert expired token");

        store
            .insert_at("current".to_string(), prepared("current.xlsx"), now)
            .expect("insert current token");

        assert!(!store.pending.contains_key("expired"));
        assert!(store.pending.contains_key("current"));
        assert_eq!(store.order, VecDeque::from(["current".to_string()]));
        assert_eq!(store.take_retired().len(), 1);
    }

    #[test]
    fn taking_an_expired_token_removes_it() {
        let mut store = PreparedDocumentStore::default();
        store
            .insert_at(
                "expired".to_string(),
                prepared("expired.xlsx"),
                Instant::now() - PREPARED_DOCUMENT_TTL,
            )
            .expect("insert expired token");

        assert!(store.take("expired").is_none());
        assert!(store.pending.is_empty());
        assert!(store.order.is_empty());
        assert_eq!(store.take_retired().len(), 1);
    }

    #[test]
    fn parse_reservation_rejects_an_oversized_estimate() {
        assert!(matches!(
            validate_prepared_document_bytes(MAX_PREPARED_DOCUMENT_BYTES + 1),
            Err(AppError::ResourceLimitExceeded(_))
        ));
    }

    #[test]
    fn combined_budget_uses_the_supplied_active_document_estimate() {
        assert!(
            validate_combined_document_bytes_for_active(
                MAX_ACTIVE_AND_PREPARED_DOCUMENT_BYTES - 1,
                1,
            )
            .is_ok()
        );
        assert!(matches!(
            validate_combined_document_bytes_for_active(MAX_ACTIVE_AND_PREPARED_DOCUMENT_BYTES, 1,),
            Err(AppError::ResourceLimitExceeded(_))
        ));
    }
}
