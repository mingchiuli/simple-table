use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::application::document_work_budget_port::DocumentWorkLease;
use crate::error::AppError;
use crate::resource_limits::{
    validate_active_and_prepared_document_bytes, validate_prepared_document_bytes,
};
use crate::state::editor_state::EditorState;

pub(crate) struct PreparedDocument {
    pub(crate) editor_state: EditorState,
    pub(crate) source_path: Option<PathBuf>,
    _work: Option<Box<dyn DocumentWorkLease>>,
}

#[derive(Default)]
struct PreparedDocumentStore {
    pending: HashMap<String, PreparedDocumentEntry>,
    order: VecDeque<String>,
    retired: Vec<PreparedDocument>,
    prepare_in_progress: Option<InProgressPreparation>,
    terminal_preparations: HashMap<String, TerminalPreparation>,
    terminal_order: VecDeque<String>,
    checkout_in_progress: bool,
}

struct PreparedDocumentEntry {
    document: PreparedDocument,
    fingerprint: PreparedDocumentFingerprint,
    created_at: Instant,
}

struct InProgressPreparation {
    token: String,
    fingerprint: PreparedDocumentFingerprint,
}

struct TerminalPreparation {
    fingerprint: Option<PreparedDocumentFingerprint>,
    outcome: TerminalPreparationOutcome,
    created_at: Instant,
}

#[derive(Clone, Copy)]
enum TerminalPreparationOutcome {
    Cancelled,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PreparedDocumentFingerprint([u8; 32]);

impl PreparedDocumentFingerprint {
    pub(crate) fn open(source_identity: &str) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"open\0");
        hash_text(&mut digest, source_identity);
        Self(digest.finalize().into())
    }

    pub(crate) fn new_file() -> Self {
        Self(Sha256::digest(b"new\0").into())
    }
}

#[derive(Debug)]
enum PrepareAdmission {
    Execute,
    Replay,
    Wait,
}

impl PreparedDocumentStore {
    fn begin_prepare(
        &mut self,
        token: &str,
        fingerprint: PreparedDocumentFingerprint,
        now: Instant,
    ) -> Result<PrepareAdmission, AppError> {
        self.prune_expired(now);
        validate_preparation_id(token)?;
        if let Some(terminal) = self.terminal_preparations.get(token) {
            if let Some(current) = terminal.fingerprint {
                ensure_same_fingerprint(current, fingerprint)?;
            }
            return Err(terminal_preparation_error(terminal.outcome));
        }
        if let Some(entry) = self.pending.get(token) {
            ensure_same_fingerprint(entry.fingerprint, fingerprint)?;
            return Ok(PrepareAdmission::Replay);
        }
        if let Some(in_progress) = &self.prepare_in_progress {
            if in_progress.token == token {
                ensure_same_fingerprint(in_progress.fingerprint, fingerprint)?;
                return Ok(PrepareAdmission::Wait);
            }
            return Err(AppError::PreparedDocumentConflict);
        }
        if self.checkout_in_progress || self.pending.len() >= MAX_PREPARED_DOCUMENTS {
            return Err(AppError::PreparedDocumentConflict);
        }
        self.prepare_in_progress = Some(InProgressPreparation {
            token: token.to_string(),
            fingerprint,
        });
        Ok(PrepareAdmission::Execute)
    }

    fn finish_prepare(&mut self, token: &str) {
        if self
            .prepare_in_progress
            .as_ref()
            .is_some_and(|preparation| preparation.token == token)
        {
            self.prepare_in_progress = None;
        }
    }

    fn fail_prepare(&mut self, token: &str, fingerprint: PreparedDocumentFingerprint) {
        if self
            .prepare_in_progress
            .as_ref()
            .is_none_or(|preparation| preparation.token != token)
        {
            return;
        }
        self.finish_prepare(token);
        if !self.terminal_preparations.contains_key(token) {
            self.record_terminal(
                token.to_string(),
                Some(fingerprint),
                TerminalPreparationOutcome::Failed,
                Instant::now(),
            );
        }
    }

    fn insert(
        &mut self,
        token: String,
        fingerprint: PreparedDocumentFingerprint,
        prepared: PreparedDocument,
    ) -> Result<(), AppError> {
        self.insert_at(token, fingerprint, prepared, Instant::now())
    }

    fn insert_at(
        &mut self,
        token: String,
        fingerprint: PreparedDocumentFingerprint,
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
                fingerprint,
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
        let fingerprint = self
            .prepare_in_progress
            .as_ref()
            .filter(|preparation| preparation.token == token)
            .map(|preparation| preparation.fingerprint)
            .or_else(|| self.pending.get(token).map(|entry| entry.fingerprint))
            .or_else(|| {
                self.terminal_preparations
                    .get(token)
                    .and_then(|terminal| terminal.fingerprint)
            });
        if let Some(entry) = self.pending.remove(token) {
            self.retired.push(entry.document);
        }
        self.order.retain(|pending_token| pending_token != token);
        self.record_terminal(
            token.to_string(),
            fingerprint,
            TerminalPreparationOutcome::Cancelled,
            Instant::now(),
        );
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
        let expired_terminal: Vec<_> = self
            .terminal_preparations
            .iter()
            .filter(|(_, terminal)| {
                now.saturating_duration_since(terminal.created_at) >= PREPARATION_TERMINAL_TTL
            })
            .map(|(token, _)| token.clone())
            .collect();
        for token in expired_terminal {
            self.terminal_preparations.remove(&token);
        }
        self.terminal_order
            .retain(|token| self.terminal_preparations.contains_key(token));
    }

    fn record_terminal(
        &mut self,
        token: String,
        fingerprint: Option<PreparedDocumentFingerprint>,
        outcome: TerminalPreparationOutcome,
        now: Instant,
    ) {
        self.terminal_order.retain(|current| current != &token);
        self.terminal_order.push_back(token.clone());
        self.terminal_preparations.insert(
            token,
            TerminalPreparation {
                fingerprint,
                outcome,
                created_at: now,
            },
        );
        while self.terminal_order.len() > MAX_TERMINAL_PREPARATIONS {
            if let Some(expired) = self.terminal_order.pop_front() {
                self.terminal_preparations.remove(&expired);
            }
        }
    }

    fn take_retired(&mut self) -> Vec<PreparedDocument> {
        std::mem::take(&mut self.retired)
    }
}

const MAX_PREPARED_DOCUMENTS: usize = 1;
const PREPARED_DOCUMENT_TTL: Duration = Duration::from_secs(5 * 60);
const PREPARATION_TERMINAL_TTL: Duration = PREPARED_DOCUMENT_TTL;
const MAX_TERMINAL_PREPARATIONS: usize = 64;
const MAX_PREPARATION_ID_BYTES: usize = 128;

#[derive(Default)]
struct PreparedDocumentRepositoryInner {
    store: Mutex<PreparedDocumentStore>,
    prepare_completed: Condvar,
}

#[derive(Clone, Default)]
pub(crate) struct PreparedDocumentRepository {
    inner: Arc<PreparedDocumentRepositoryInner>,
}

pub(crate) struct PrepareReservation {
    repository: PreparedDocumentRepository,
    token: String,
    fingerprint: PreparedDocumentFingerprint,
    active: bool,
}

pub(crate) enum PrepareReservationResult {
    Execute(PrepareReservation),
    Replay,
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
        let retired = match self.repository.inner.store.lock() {
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
        if let Ok(mut store) = self.repository.inner.store.lock() {
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
        if let Ok(mut store) = self.repository.inner.store.lock() {
            store.fail_prepare(&self.token, self.fingerprint);
        }
        self.repository.inner.prepare_completed.notify_all();
    }
}

impl PreparedDocumentRepository {
    #[cfg(test)]
    pub(crate) fn is_same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    pub(crate) fn reserve(
        &self,
        token: &str,
        fingerprint: PreparedDocumentFingerprint,
    ) -> Result<PrepareReservationResult, AppError> {
        let mut store = self
            .inner
            .store
            .lock()
            .map_err(|_| AppError::poisoned_lock("prepared document store"))?;
        loop {
            let result = store.begin_prepare(token, fingerprint, Instant::now());
            let retired = store.take_retired();
            let result = match result {
                Ok(result) => result,
                Err(error) => {
                    drop(store);
                    drop(retired);
                    return Err(error);
                }
            };
            match result {
                PrepareAdmission::Execute => {
                    let reservation = PrepareReservation {
                        repository: self.clone(),
                        token: token.to_string(),
                        fingerprint,
                        active: true,
                    };
                    drop(store);
                    drop(retired);
                    return Ok(PrepareReservationResult::Execute(reservation));
                }
                PrepareAdmission::Replay => {
                    drop(store);
                    drop(retired);
                    return Ok(PrepareReservationResult::Replay);
                }
                PrepareAdmission::Wait => {
                    if !retired.is_empty() {
                        drop(store);
                        drop(retired);
                        store = self
                            .inner
                            .store
                            .lock()
                            .map_err(|_| AppError::poisoned_lock("prepared document store"))?;
                        continue;
                    }
                    store = self
                        .inner
                        .prepare_completed
                        .wait(store)
                        .map_err(|_| AppError::poisoned_lock("prepared document store"))?;
                }
            }
        }
    }
}

impl PreparedDocumentRepository {
    pub(crate) fn replace(
        &self,
        editor_state: EditorState,
        source_path: Option<PathBuf>,
        work: Box<dyn DocumentWorkLease>,
        mut reservation: PrepareReservation,
        active_document_bytes: usize,
    ) -> Result<String, AppError> {
        let estimated_bytes = editor_state.estimated_resource_bytes();
        validate_prepared_document_bytes(estimated_bytes)?;
        let token = reservation.token.clone();
        let fingerprint = reservation.fingerprint;
        let prepared = PreparedDocument {
            editor_state,
            source_path,
            _work: Some(work),
        };
        validate_active_and_prepared_document_bytes(active_document_bytes, estimated_bytes)?;
        let (result, retired) = {
            let mut store = self
                .inner
                .store
                .lock()
                .map_err(|_| AppError::poisoned_lock("prepared document store"))?;
            store.finish_prepare(&token);
            reservation.active = false;
            let result = if let Some(terminal) = store.terminal_preparations.get(&token) {
                let error = terminal_preparation_error(terminal.outcome);
                store.retired.push(prepared);
                Err(error)
            } else {
                store.insert(token.clone(), fingerprint, prepared)
            };
            (result, store.take_retired())
        };
        self.inner.prepare_completed.notify_all();
        drop(retired);
        result?;
        Ok(token)
    }
}

impl PreparedDocumentRepository {
    #[cfg(test)]
    pub(crate) fn take(&self, token: &str) -> Result<PreparedDocument, AppError> {
        let (prepared, retired) = {
            let mut store = self
                .inner
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
                .inner
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
                .inner
                .store
                .lock()
                .map_err(|_| AppError::poisoned_lock("prepared document store"))?;
            store.abort(token);
            store.take_retired()
        };
        drop(retired);
        self.inner.prepare_completed.notify_all();
        Ok(())
    }

    pub(crate) fn project<T>(
        &self,
        token: &str,
        fingerprint: PreparedDocumentFingerprint,
        project: impl FnOnce(&EditorState) -> T,
    ) -> Result<T, AppError> {
        let (result, retired) = {
            let mut store = self
                .inner
                .store
                .lock()
                .map_err(|_| AppError::poisoned_lock("prepared document store"))?;
            store.prune_expired(Instant::now());
            let result = (|| {
                let entry = store.pending.get(token).ok_or_else(|| {
                    AppError::DocumentStateInvalid(
                        "prepared document token is no longer active".to_string(),
                    )
                })?;
                ensure_same_fingerprint(entry.fingerprint, fingerprint)?;
                Ok(project(&entry.document.editor_state))
            })();
            (result, store.take_retired())
        };
        drop(retired);
        result
    }
}

fn validate_preparation_id(token: &str) -> Result<(), AppError> {
    if token.is_empty() || token.len() > MAX_PREPARATION_ID_BYTES {
        return Err(AppError::DocumentStateInvalid(
            "preparationId must contain between 1 and 128 bytes".to_string(),
        ));
    }
    Ok(())
}

fn ensure_same_fingerprint(
    current: PreparedDocumentFingerprint,
    requested: PreparedDocumentFingerprint,
) -> Result<(), AppError> {
    if current == requested {
        return Ok(());
    }
    Err(AppError::DocumentStateInvalid(
        "preparationId was reused with a different source".to_string(),
    ))
}

fn preparation_aborted_error() -> AppError {
    AppError::DocumentStateInvalid("document preparation was aborted".to_string())
}

fn preparation_failed_error() -> AppError {
    AppError::DocumentStateInvalid("document preparation previously failed".to_string())
}

fn terminal_preparation_error(outcome: TerminalPreparationOutcome) -> AppError {
    match outcome {
        TerminalPreparationOutcome::Cancelled => preparation_aborted_error(),
        TerminalPreparationOutcome::Failed => preparation_failed_error(),
    }
}

fn hash_text(digest: &mut Sha256, value: &str) {
    digest.update((value.len() as u64).to_le_bytes());
    digest.update(value.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_data::{DocumentData, DocumentSheet};
    use crate::resource_limits::{
        MAX_ACTIVE_AND_PREPARED_DOCUMENT_BYTES, MAX_PREPARED_DOCUMENT_BYTES,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread;

    struct NoopWorkLease;

    impl DocumentWorkLease for NoopWorkLease {
        fn set_work_bytes(&mut self, _work_bytes: usize) -> Result<(), AppError> {
            Ok(())
        }
    }

    struct DropTrackingWorkLease(Arc<AtomicBool>);

    impl DocumentWorkLease for DropTrackingWorkLease {
        fn set_work_bytes(&mut self, _work_bytes: usize) -> Result<(), AppError> {
            Ok(())
        }
    }

    impl Drop for DropTrackingWorkLease {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    fn prepared(name: &str) -> PreparedDocument {
        PreparedDocument {
            editor_state: EditorState::with_workbook(
                DocumentData {
                    path: String::new(),
                    file_name: name.to_string(),
                    sheets: vec![DocumentSheet::default()],
                },
                None,
            ),
            source_path: None,
            _work: None,
        }
    }

    fn fingerprint(value: &str) -> PreparedDocumentFingerprint {
        PreparedDocumentFingerprint::open(value)
    }

    #[test]
    fn taking_a_token_consumes_it() {
        let mut store = PreparedDocumentStore::default();
        store
            .insert(
                "token".to_string(),
                fingerprint("token"),
                prepared("book.xlsx"),
            )
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
            .insert(
                "token".to_string(),
                fingerprint("token"),
                prepared("book.xlsx"),
            )
            .expect("insert token");
        let entry = store.checkout("token").expect("checkout");
        assert!(
            store
                .begin_prepare("other", fingerprint("other"), Instant::now())
                .is_err()
        );

        store.restore_checkout("token".to_string(), entry);

        assert!(store.take("token").is_some());
        assert!(!store.checkout_in_progress);
    }

    #[test]
    fn abort_is_idempotent() {
        let mut store = PreparedDocumentStore::default();
        store
            .insert(
                "token".to_string(),
                fingerprint("token"),
                prepared("book.xlsx"),
            )
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
            .insert(
                "token-0".to_string(),
                fingerprint("token-0"),
                prepared("book-0.xlsx"),
            )
            .expect("insert first token");

        assert!(matches!(
            store.insert(
                "token-1".to_string(),
                fingerprint("token-1"),
                prepared("book-1.xlsx"),
            ),
            Err(AppError::PreparedDocumentConflict)
        ));
        assert_eq!(store.take_retired().len(), 1);
        assert!(store.take("token-0").is_some());
        assert!(store.take("token-1").is_none());
    }

    #[test]
    fn preparation_reservation_waits_for_matching_retry_and_rejects_other_work() {
        let mut store = PreparedDocumentStore::default();

        store
            .begin_prepare("token", fingerprint("token"), Instant::now())
            .expect("reserve prepare");

        assert!(matches!(
            store.begin_prepare("token", fingerprint("token"), Instant::now()),
            Ok(PrepareAdmission::Wait)
        ));
        assert!(matches!(
            store.begin_prepare("other", fingerprint("other"), Instant::now()),
            Err(AppError::PreparedDocumentConflict)
        ));
        store.finish_prepare("token");
        assert!(matches!(
            store.begin_prepare("other", fingerprint("other"), Instant::now()),
            Ok(PrepareAdmission::Execute)
        ));
    }

    #[test]
    fn completed_preparation_replays_only_for_the_same_fingerprint() {
        let mut store = PreparedDocumentStore::default();
        store
            .insert(
                "token".to_string(),
                fingerprint("source"),
                prepared("book.xlsx"),
            )
            .expect("insert prepared document");

        assert!(matches!(
            store.begin_prepare("token", fingerprint("source"), Instant::now()),
            Ok(PrepareAdmission::Replay)
        ));
        assert!(matches!(
            store.begin_prepare("token", fingerprint("other"), Instant::now()),
            Err(AppError::DocumentStateInvalid(_))
        ));
    }

    #[test]
    fn aborting_an_in_progress_preparation_prevents_its_result_from_being_reused() {
        let mut store = PreparedDocumentStore::default();
        assert!(matches!(
            store.begin_prepare("token", fingerprint("source"), Instant::now()),
            Ok(PrepareAdmission::Execute)
        ));

        store.abort("token");
        store.finish_prepare("token");

        assert!(matches!(
            store.begin_prepare("token", fingerprint("source"), Instant::now()),
            Err(AppError::DocumentStateInvalid(message))
                if message == "document preparation was aborted"
        ));
        assert!(matches!(
            store.begin_prepare("token", fingerprint("source"), Instant::now()),
            Err(AppError::DocumentStateInvalid(message))
                if message == "document preparation was aborted"
        ));
        assert!(matches!(
            store.begin_prepare("token", fingerprint("other"), Instant::now()),
            Err(AppError::DocumentStateInvalid(message))
                if message == "preparationId was reused with a different source"
        ));
    }

    #[test]
    fn an_unknown_abort_prevents_a_later_preparation_with_the_same_id() {
        let mut store = PreparedDocumentStore::default();

        store.abort("token");

        assert!(matches!(
            store.begin_prepare("token", fingerprint("source"), Instant::now()),
            Err(AppError::DocumentStateInvalid(message))
                if message == "document preparation was aborted"
        ));
    }

    #[test]
    fn dropping_a_reservation_records_a_stable_failed_terminal_state() {
        let repository = PreparedDocumentRepository::default();
        let reservation = match repository
            .reserve("failed", fingerprint("source"))
            .expect("reserve preparation")
        {
            PrepareReservationResult::Execute(reservation) => reservation,
            PrepareReservationResult::Replay => panic!("first request cannot replay"),
        };

        drop(reservation);

        for _ in 0..2 {
            assert!(matches!(
                repository.reserve("failed", fingerprint("source")),
                Err(AppError::DocumentStateInvalid(message))
                    if message == "document preparation previously failed"
            ));
        }
        assert!(matches!(
            repository.reserve("failed", fingerprint("other")),
            Err(AppError::DocumentStateInvalid(message))
                if message == "preparationId was reused with a different source"
        ));
    }

    #[test]
    fn retry_cannot_consume_cancellation_before_a_late_result_arrives() {
        let repository = PreparedDocumentRepository::default();
        let fingerprint = fingerprint("source");
        let reservation = match repository
            .reserve("cancelled", fingerprint)
            .expect("reserve preparation")
        {
            PrepareReservationResult::Execute(reservation) => reservation,
            PrepareReservationResult::Replay => panic!("first request cannot replay"),
        };

        repository.abort("cancelled").expect("abort preparation");
        assert!(matches!(
            repository.reserve("cancelled", fingerprint),
            Err(AppError::DocumentStateInvalid(message))
                if message == "document preparation was aborted"
        ));

        assert!(matches!(
            repository.replace(
                prepared("late.xlsx").editor_state,
                None,
                Box::new(NoopWorkLease),
                reservation,
                0,
            ),
            Err(AppError::DocumentStateInvalid(message))
                if message == "document preparation was aborted"
        ));
        assert!(repository.take("cancelled").is_err());
    }

    #[test]
    fn concurrent_retry_waits_and_replays_the_completed_preparation() {
        let repository = PreparedDocumentRepository::default();
        let fingerprint = fingerprint("source");
        let reservation = match repository
            .reserve("shared", fingerprint)
            .expect("first reservation")
        {
            PrepareReservationResult::Execute(reservation) => reservation,
            PrepareReservationResult::Replay => panic!("first request cannot replay"),
        };
        let retry_repository = repository.clone();
        let (sender, receiver) = mpsc::channel();
        let retry = thread::spawn(move || {
            sender
                .send(retry_repository.reserve("shared", fingerprint))
                .expect("send retry result");
        });
        assert!(receiver.recv_timeout(Duration::from_millis(20)).is_err());

        repository
            .replace(
                prepared("book.xlsx").editor_state,
                None,
                Box::new(NoopWorkLease),
                reservation,
                0,
            )
            .expect("complete preparation");

        assert!(matches!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("retry unblocks")
                .expect("retry succeeds"),
            PrepareReservationResult::Replay
        ));
        retry.join().expect("retry thread");
    }

    #[test]
    fn insertion_prunes_expired_documents_before_enforcing_capacity() {
        let mut store = PreparedDocumentStore::default();
        let now = Instant::now();
        store
            .insert_at(
                "expired".to_string(),
                fingerprint("expired"),
                prepared("expired.xlsx"),
                now - PREPARED_DOCUMENT_TTL,
            )
            .expect("insert expired token");

        store
            .insert_at(
                "current".to_string(),
                fingerprint("current"),
                prepared("current.xlsx"),
                now,
            )
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
                fingerprint("expired"),
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
    fn projecting_an_expired_token_releases_its_work_lease() {
        let repository = PreparedDocumentRepository::default();
        let lease_dropped = Arc::new(AtomicBool::new(false));
        let mut document = prepared("expired.xlsx");
        document._work = Some(Box::new(DropTrackingWorkLease(lease_dropped.clone())));
        repository
            .inner
            .store
            .lock()
            .expect("prepared document store")
            .insert_at(
                "expired".to_string(),
                fingerprint("expired"),
                document,
                Instant::now() - PREPARED_DOCUMENT_TTL,
            )
            .expect("insert expired token");

        assert!(
            repository
                .project("expired", fingerprint("expired"), |_| ())
                .is_err()
        );

        assert!(lease_dropped.load(Ordering::SeqCst));
        assert!(
            repository
                .inner
                .store
                .lock()
                .expect("prepared document store")
                .retired
                .is_empty()
        );
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
            validate_active_and_prepared_document_bytes(
                MAX_ACTIVE_AND_PREPARED_DOCUMENT_BYTES - 1,
                1,
            )
            .is_ok()
        );
        assert!(matches!(
            validate_active_and_prepared_document_bytes(MAX_ACTIVE_AND_PREPARED_DOCUMENT_BYTES, 1,),
            Err(AppError::ResourceLimitExceeded(_))
        ));
    }
}
