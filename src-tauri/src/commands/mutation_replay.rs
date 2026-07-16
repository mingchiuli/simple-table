use std::collections::{HashSet, VecDeque};
use std::io::{self, Write};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::AppError;
use crate::types::{
    EditorMutationResponse, EditorPatch, MutationResultLookup, ResyncRequiredPatch,
};

const MAX_REPLAY_ENTRIES: usize = 128;
const MAX_REPLAY_BYTES: usize = 4 * 1024 * 1024;
const MAX_IN_FLIGHT_MUTATIONS: usize = 64;
const MAX_COMMAND_ID_BYTES: usize = 128;

type RequestFingerprint = [u8; 32];

#[derive(Clone)]
struct ReplayEntry {
    document_id: u64,
    command_id: String,
    fingerprint: RequestFingerprint,
    response: Arc<EditorMutationResponse>,
    bytes: usize,
}

struct InFlightMutation {
    document_id: u64,
    command_id: String,
    fingerprint: RequestFingerprint,
}

#[derive(Default)]
struct MutationReplayCache {
    entries: VecDeque<ReplayEntry>,
    in_flight: Vec<InFlightMutation>,
    retired_documents: HashSet<u64>,
    bytes: usize,
}

#[derive(Default)]
struct MutationReplayCoordinator {
    cache: Mutex<MutationReplayCache>,
    completed: Condvar,
}

static MUTATION_REPLAYS: OnceLock<MutationReplayCoordinator> = OnceLock::new();

pub(crate) fn run<P: Serialize>(
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    command_name: &str,
    payload: &P,
    execute: impl FnOnce() -> Result<EditorMutationResponse, AppError>,
) -> Result<EditorMutationResponse, AppError> {
    validate_command_id(command_id)?;
    let fingerprint = request_fingerprint(base_revision, command_name, payload)?;
    let reservation = match reserve(document_id, command_id, fingerprint)? {
        ReservationResult::Replay(response) => return Ok((*response).clone()),
        ReservationResult::Execute(reservation) => reservation,
    };

    let result = execute();
    reservation.finish(result)
}

enum ReservationResult {
    Replay(Arc<EditorMutationResponse>),
    Execute(InFlightReservation),
}

struct InFlightReservation {
    coordinator: &'static MutationReplayCoordinator,
    document_id: u64,
    command_id: String,
    fingerprint: RequestFingerprint,
    finished: bool,
}

impl InFlightReservation {
    fn finish(
        mut self,
        result: Result<EditorMutationResponse, AppError>,
    ) -> Result<EditorMutationResponse, AppError> {
        let prepared = result
            .as_ref()
            .ok()
            .and_then(|response| prepare_replay_response(&self.command_id, response));
        let mut cache = lock_cache(self.coordinator)?;
        if let Some(prepared) =
            prepared.filter(|_| !cache.retired_documents.contains(&self.document_id))
        {
            insert_response(
                &mut cache,
                self.document_id,
                &self.command_id,
                self.fingerprint,
                prepared,
            );
        }
        remove_in_flight(&mut cache, self.document_id, &self.command_id);
        finish_retirement(&mut cache, self.document_id);
        self.finished = true;
        self.coordinator.completed.notify_all();
        result
    }
}

impl Drop for InFlightReservation {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let mut cache = self
            .coordinator
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        remove_in_flight(&mut cache, self.document_id, &self.command_id);
        finish_retirement(&mut cache, self.document_id);
        self.coordinator.completed.notify_all();
    }
}

fn reserve(
    document_id: u64,
    command_id: &str,
    fingerprint: RequestFingerprint,
) -> Result<ReservationResult, AppError> {
    reserve_with_coordinator(replay_coordinator(), document_id, command_id, fingerprint)
}

fn reserve_with_coordinator(
    coordinator: &'static MutationReplayCoordinator,
    document_id: u64,
    command_id: &str,
    fingerprint: RequestFingerprint,
) -> Result<ReservationResult, AppError> {
    let mut cache = lock_cache(coordinator)?;
    loop {
        if cache.retired_documents.contains(&document_id) {
            return Err(AppError::DocumentStateInvalid(
                "mutation command belongs to a document that is closing".to_string(),
            ));
        }
        if let Some(entry) = find_entry(&cache, document_id, command_id) {
            if entry.fingerprint != fingerprint {
                return Err(reused_command_id_error());
            }
            return Ok(ReservationResult::Replay(Arc::clone(&entry.response)));
        }

        if let Some(in_flight) = cache
            .in_flight
            .iter()
            .find(|entry| entry.document_id == document_id && entry.command_id == command_id)
        {
            if in_flight.fingerprint != fingerprint {
                return Err(reused_command_id_error());
            }
            cache = wait_for_completion(coordinator, cache)?;
            continue;
        }

        if cache.in_flight.len() >= MAX_IN_FLIGHT_MUTATIONS {
            return Err(AppError::ResourceLimitExceeded(format!(
                "at most {MAX_IN_FLIGHT_MUTATIONS} mutation commands may be in flight"
            )));
        }
        cache.in_flight.push(InFlightMutation {
            document_id,
            command_id: command_id.to_string(),
            fingerprint,
        });
        return Ok(ReservationResult::Execute(InFlightReservation {
            coordinator,
            document_id,
            command_id: command_id.to_string(),
            fingerprint,
            finished: false,
        }));
    }
}

fn insert_response(
    cache: &mut MutationReplayCache,
    document_id: u64,
    command_id: &str,
    fingerprint: RequestFingerprint,
    prepared: PreparedReplayResponse,
) {
    while cache.entries.len() >= MAX_REPLAY_ENTRIES
        || cache.bytes.saturating_add(prepared.bytes) > MAX_REPLAY_BYTES
    {
        let Some(expired) = cache.entries.pop_front() else {
            break;
        };
        cache.bytes = cache.bytes.saturating_sub(expired.bytes);
    }
    cache.bytes = cache.bytes.saturating_add(prepared.bytes);
    cache.entries.push_back(ReplayEntry {
        document_id,
        command_id: command_id.to_string(),
        fingerprint,
        response: Arc::new(prepared.response),
        bytes: prepared.bytes,
    });
}

struct PreparedReplayResponse {
    response: EditorMutationResponse,
    bytes: usize,
}

fn prepare_replay_response(
    command_id: &str,
    response: &EditorMutationResponse,
) -> Option<PreparedReplayResponse> {
    let original_bytes = serde_json::to_vec(response).map_or(usize::MAX, |value| value.len());
    let (response, response_bytes) = if original_bytes <= MAX_REPLAY_BYTES {
        (response.clone(), original_bytes)
    } else {
        let response = compact_replay_response(response);
        let bytes = serde_json::to_vec(&response)
            .map(|value| value.len())
            .unwrap_or(MAX_REPLAY_BYTES.saturating_add(1));
        (response, bytes)
    };
    let bytes = response_bytes
        .saturating_add(command_id.len())
        .saturating_add(std::mem::size_of::<RequestFingerprint>())
        .saturating_add(std::mem::size_of::<ReplayEntry>());
    (bytes <= MAX_REPLAY_BYTES).then_some(PreparedReplayResponse { response, bytes })
}

fn compact_replay_response(response: &EditorMutationResponse) -> EditorMutationResponse {
    let mut compact = response.clone();
    compact.patches = vec![EditorPatch::ResyncRequired {
        patch: ResyncRequiredPatch {
            reason: "mutation response exceeded replay budget".to_string(),
        },
    }];
    compact.sheet_layouts = None;
    compact
}

pub(crate) fn retire_document(document_id: u64) {
    let _ = retire_document_with_coordinator(replay_coordinator(), document_id);
}

fn retire_document_with_coordinator(
    coordinator: &MutationReplayCoordinator,
    document_id: u64,
) -> Result<(), AppError> {
    let mut cache = lock_cache(coordinator)?;
    cache
        .entries
        .retain(|entry| entry.document_id != document_id);
    cache.bytes = cache.entries.iter().map(|entry| entry.bytes).sum();
    if cache
        .in_flight
        .iter()
        .any(|entry| entry.document_id == document_id)
    {
        cache.retired_documents.insert(document_id);
    } else {
        cache.retired_documents.remove(&document_id);
    }
    Ok(())
}

pub(crate) fn get(document_id: u64, command_id: &str) -> Result<MutationResultLookup, AppError> {
    validate_command_id(command_id)?;
    let coordinator = replay_coordinator();
    let cache = lock_cache(coordinator)?;
    if cache
        .in_flight
        .iter()
        .any(|entry| entry.document_id == document_id && entry.command_id == command_id)
    {
        return Ok(MutationResultLookup::pending());
    }
    let response =
        find_entry(&cache, document_id, command_id).map(|entry| Arc::clone(&entry.response));
    drop(cache);
    Ok(
        response.map_or_else(MutationResultLookup::missing, |response| {
            MutationResultLookup::completed((*response).clone())
        }),
    )
}

fn find_entry<'a>(
    cache: &'a MutationReplayCache,
    document_id: u64,
    command_id: &str,
) -> Option<&'a ReplayEntry> {
    cache
        .entries
        .iter()
        .find(|entry| entry.document_id == document_id && entry.command_id == command_id)
}

fn remove_in_flight(cache: &mut MutationReplayCache, document_id: u64, command_id: &str) {
    cache
        .in_flight
        .retain(|entry| entry.document_id != document_id || entry.command_id != command_id);
}

fn finish_retirement(cache: &mut MutationReplayCache, document_id: u64) {
    if cache.retired_documents.contains(&document_id)
        && !cache
            .in_flight
            .iter()
            .any(|entry| entry.document_id == document_id)
    {
        cache.retired_documents.remove(&document_id);
    }
}

fn request_fingerprint<P: Serialize>(
    base_revision: u64,
    command_name: &str,
    payload: &P,
) -> Result<RequestFingerprint, AppError> {
    let mut hasher = Sha256::new();
    hasher.update(base_revision.to_le_bytes());
    hasher.update(command_name.len().to_le_bytes());
    hasher.update(command_name.as_bytes());
    serde_json::to_writer(DigestWriter(&mut hasher), payload)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    Ok(hasher.finalize().into())
}

struct DigestWriter<'a>(&'a mut Sha256);

impl Write for DigestWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn validate_command_id(command_id: &str) -> Result<(), AppError> {
    if command_id.is_empty() || command_id.len() > MAX_COMMAND_ID_BYTES {
        return Err(AppError::DocumentStateInvalid(
            "mutation commandId must contain between 1 and 128 bytes".to_string(),
        ));
    }
    Ok(())
}

fn reused_command_id_error() -> AppError {
    AppError::DocumentStateInvalid(
        "mutation commandId was reused with a different payload".to_string(),
    )
}

fn lock_cache(
    coordinator: &MutationReplayCoordinator,
) -> Result<MutexGuard<'_, MutationReplayCache>, AppError> {
    coordinator
        .cache
        .lock()
        .map_err(|_| AppError::poisoned_lock("mutation replay cache"))
}

fn wait_for_completion<'a>(
    coordinator: &MutationReplayCoordinator,
    cache: MutexGuard<'a, MutationReplayCache>,
) -> Result<MutexGuard<'a, MutationReplayCache>, AppError> {
    coordinator
        .completed
        .wait(cache)
        .map_err(|_| AppError::poisoned_lock("mutation replay cache"))
}

fn replay_coordinator() -> &'static MutationReplayCoordinator {
    MUTATION_REPLAYS.get_or_init(MutationReplayCoordinator::default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::patch_projector::status_mutation_response;
    use crate::state::editor_state::EditorState;
    use crate::types::FileData;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, mpsc};
    use std::thread;
    use std::time::Duration;

    fn response() -> EditorMutationResponse {
        let state = EditorState::with_workbook(
            FileData {
                path: String::new(),
                file_name: "book.xlsx".to_string(),
                sheets: Vec::new(),
            },
            None,
        );
        status_mutation_response(&state)
    }

    #[test]
    fn replays_successful_mutations_once() {
        let calls = AtomicUsize::new(0);
        let first = run(91, 0, "command", "set_cell", &(0, 0), || {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(response())
        })
        .expect("first mutation");
        let second = run(91, 0, "command", "set_cell", &(0, 0), || {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(response())
        })
        .expect("replayed mutation");

        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(first.revision, second.revision);
        retire_document(91);
    }

    #[test]
    fn concurrent_retries_share_one_execution() {
        let document_id = 92;
        let started = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let calls = Arc::new(AtomicUsize::new(0));

        let first = {
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            let calls = Arc::clone(&calls);
            thread::spawn(move || {
                run(document_id, 0, "shared", "set_cell", &(0, 0), || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    started.wait();
                    release.wait();
                    Ok(response())
                })
            })
        };
        started.wait();
        let second = {
            let calls = Arc::clone(&calls);
            thread::spawn(move || {
                run(document_id, 0, "shared", "set_cell", &(0, 0), || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(response())
                })
            })
        };

        thread::sleep(Duration::from_millis(20));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        release.wait();
        assert!(first.join().expect("first caller").is_ok());
        assert!(second.join().expect("retry caller").is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        retire_document(document_id);
    }

    #[test]
    fn unrelated_result_queries_do_not_wait_for_a_running_mutation() {
        let document_id = 93;
        let started = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let mutation = {
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            thread::spawn(move || {
                run(document_id, 0, "running", "set_cell", &(0, 0), || {
                    started.wait();
                    release.wait();
                    Ok(response())
                })
            })
        };
        started.wait();

        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || sender.send(get(94, "unrelated")).expect("query result"));
        let result = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("unrelated query should not block")
            .expect("query succeeds");
        assert_eq!(result.status, crate::types::MutationResultStatus::Missing);

        release.wait();
        assert!(mutation.join().expect("mutation caller").is_ok());
        retire_document(document_id);
    }

    #[test]
    fn matching_result_query_reports_pending_without_waiting() {
        let document_id = 95;
        let started = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let mutation = {
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            thread::spawn(move || {
                run(document_id, 0, "running", "set_cell", &(0, 0), || {
                    started.wait();
                    release.wait();
                    Ok(response())
                })
            })
        };
        started.wait();

        let lookup = get(document_id, "running").expect("query result");
        assert_eq!(lookup.status, crate::types::MutationResultStatus::Pending);
        release.wait();
        assert!(mutation.join().expect("mutation caller").is_ok());
        let lookup = get(document_id, "running").expect("completed query result");
        assert_eq!(lookup.status, crate::types::MutationResultStatus::Completed);
        assert!(lookup.response.is_some());
        retire_document(document_id);
    }

    #[test]
    fn an_in_flight_command_id_rejects_a_different_payload() {
        let document_id = 96;
        let started = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let mutation = {
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            thread::spawn(move || {
                run(document_id, 0, "running", "set_cell", &(0, 0), || {
                    started.wait();
                    release.wait();
                    Ok(response())
                })
            })
        };
        started.wait();

        let error = run(document_id, 0, "running", "set_cell", &(1, 0), || {
            Ok(response())
        })
        .expect_err("different payload must be rejected");
        assert!(matches!(error, AppError::DocumentStateInvalid(_)));

        release.wait();
        assert!(mutation.join().expect("mutation caller").is_ok());
        retire_document(document_id);
    }

    #[test]
    fn request_fingerprints_are_fixed_size_for_large_payloads() {
        let small = request_fingerprint(0, "set_cells", &vec!["x"]).expect("small hash");
        let large = request_fingerprint(0, "set_cells", &vec!["x".repeat(1024 * 1024)])
            .expect("large hash");

        assert_eq!(small.len(), 32);
        assert_eq!(large.len(), 32);
        assert_ne!(small, large);
    }

    #[test]
    fn distinct_in_flight_mutations_are_capacity_bounded() {
        let coordinator = Box::leak(Box::new(MutationReplayCoordinator::default()));
        let mut reservations = Vec::new();
        for index in 0..MAX_IN_FLIGHT_MUTATIONS {
            let fingerprint = request_fingerprint(0, "set_cell", &index).expect("fingerprint");
            let reservation =
                reserve_with_coordinator(coordinator, 97, &format!("command-{index}"), fingerprint)
                    .expect("reservation");
            let ReservationResult::Execute(reservation) = reservation else {
                panic!("new command must reserve execution");
            };
            reservations.push(reservation);
        }

        let fingerprint = request_fingerprint(0, "set_cell", &999).expect("fingerprint");
        let error =
            match reserve_with_coordinator(coordinator, 97, "one-command-too-many", fingerprint) {
                Ok(_) => panic!("in-flight limit must be enforced"),
                Err(error) => error,
            };
        assert!(matches!(error, AppError::ResourceLimitExceeded(_)));

        drop(reservations);
        assert!(
            coordinator
                .cache
                .lock()
                .expect("cache")
                .in_flight
                .is_empty()
        );
    }

    #[test]
    fn retiring_a_document_does_not_wait_and_discards_late_results() {
        let coordinator = Box::leak(Box::new(MutationReplayCoordinator::default()));
        let fingerprint = request_fingerprint(0, "set_cell", &(0, 0)).expect("fingerprint");
        let reservation = reserve_with_coordinator(coordinator, 98, "retiring", fingerprint)
            .expect("reservation");
        let ReservationResult::Execute(reservation) = reservation else {
            panic!("new command must reserve execution");
        };

        retire_document_with_coordinator(coordinator, 98).expect("retire document");
        assert!(
            coordinator
                .cache
                .lock()
                .expect("cache")
                .retired_documents
                .contains(&98)
        );
        reservation.finish(Ok(response())).expect("mutation result");

        let cache = coordinator.cache.lock().expect("cache");
        assert!(cache.entries.is_empty());
        assert!(cache.in_flight.is_empty());
        assert!(!cache.retired_documents.contains(&98));
    }
}
