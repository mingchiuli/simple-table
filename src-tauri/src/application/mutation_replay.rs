use std::collections::{HashSet, VecDeque};
use std::io::{self, Write};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::domain::CellValue;
use crate::error::AppError;
use crate::projection_model::{MutationLookup, MutationOutcome, MutationPatch};

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
    response: Arc<MutationOutcome>,
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
pub(crate) struct MutationReplayCoordinator {
    cache: Mutex<MutationReplayCache>,
    completed: Condvar,
}

pub(crate) fn run<P: Serialize>(
    coordinator: &Arc<MutationReplayCoordinator>,
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    command_name: &str,
    payload: &P,
    execute: impl FnOnce() -> Result<MutationOutcome, AppError>,
) -> Result<MutationOutcome, AppError> {
    validate_command_id(command_id)?;
    let fingerprint = request_fingerprint(base_revision, command_name, payload)?;
    let reservation = match reserve(coordinator, document_id, command_id, fingerprint)? {
        ReservationResult::Replay(response) => return Ok((*response).clone()),
        ReservationResult::Execute(reservation) => reservation,
    };

    let result = execute();
    reservation.finish(result)
}

enum ReservationResult {
    Replay(Arc<MutationOutcome>),
    Execute(InFlightReservation),
}

struct InFlightReservation {
    coordinator: Arc<MutationReplayCoordinator>,
    document_id: u64,
    command_id: String,
    fingerprint: RequestFingerprint,
    finished: bool,
}

impl InFlightReservation {
    fn finish(
        mut self,
        result: Result<MutationOutcome, AppError>,
    ) -> Result<MutationOutcome, AppError> {
        let prepared = result
            .as_ref()
            .ok()
            .and_then(|response| prepare_replay_response(&self.command_id, response));
        let replayed_response = prepared.as_ref().map(|prepared| prepared.response.clone());
        let mut cache = lock_cache(&self.coordinator)?;
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
        match (result, replayed_response) {
            (Ok(_), Some(response)) => Ok(response),
            (result, _) => result,
        }
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
    coordinator: &Arc<MutationReplayCoordinator>,
    document_id: u64,
    command_id: &str,
    fingerprint: RequestFingerprint,
) -> Result<ReservationResult, AppError> {
    reserve_with_coordinator(coordinator, document_id, command_id, fingerprint)
}

fn reserve_with_coordinator(
    coordinator: &Arc<MutationReplayCoordinator>,
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
            coordinator: Arc::clone(coordinator),
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
    response: MutationOutcome,
    bytes: usize,
}

fn prepare_replay_response(
    command_id: &str,
    response: &MutationOutcome,
) -> Option<PreparedReplayResponse> {
    let original_bytes = estimated_mutation_outcome_bytes(response);
    let (response, response_bytes) = if original_bytes <= MAX_REPLAY_BYTES {
        (response.clone(), original_bytes)
    } else {
        let response = compact_replay_response(response);
        let bytes = estimated_mutation_outcome_bytes(&response);
        (response, bytes)
    };
    let bytes = response_bytes
        .saturating_add(command_id.len())
        .saturating_add(std::mem::size_of::<RequestFingerprint>())
        .saturating_add(std::mem::size_of::<ReplayEntry>());
    (bytes <= MAX_REPLAY_BYTES).then_some(PreparedReplayResponse { response, bytes })
}

fn compact_replay_response(response: &MutationOutcome) -> MutationOutcome {
    let mut compact = response.clone();
    compact.require_resync("mutation response exceeded replay budget");
    compact
}

pub(crate) fn retire_document(coordinator: &MutationReplayCoordinator, document_id: u64) {
    let _ = retire_document_with_coordinator(coordinator, document_id);
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

pub(crate) fn get(
    coordinator: &MutationReplayCoordinator,
    document_id: u64,
    command_id: &str,
) -> Result<MutationLookup, AppError> {
    validate_command_id(command_id)?;
    let cache = lock_cache(coordinator)?;
    if cache
        .in_flight
        .iter()
        .any(|entry| entry.document_id == document_id && entry.command_id == command_id)
    {
        return Ok(MutationLookup::pending());
    }
    let response =
        find_entry(&cache, document_id, command_id).map(|entry| Arc::clone(&entry.response));
    drop(cache);
    Ok(response.map_or_else(MutationLookup::missing, |response| {
        MutationLookup::completed((*response).clone())
    }))
}

fn estimated_mutation_outcome_bytes(response: &MutationOutcome) -> usize {
    let patch_bytes = response
        .patches
        .iter()
        .map(|patch| match patch {
            MutationPatch::Cells { changes } => changes
                .iter()
                .map(|change| {
                    96usize
                        .saturating_add(change.display.as_ref().map_or(0, String::len))
                        .saturating_add(estimated_cell_value_bytes(&change.value))
                })
                .sum(),
            MutationPatch::SheetInserted { sheet, .. } => sheet.name.len().saturating_mul(6) + 256,
            MutationPatch::SheetsReplaced { sheets, .. } => sheets
                .iter()
                .map(|sheet| sheet.name.len().saturating_mul(6) + 256)
                .sum(),
            MutationPatch::ResyncRequired { reason } => reason.len().saturating_mul(6) + 64,
            MutationPatch::Layout {
                column_widths,
                row_heights,
                ..
            } => column_widths
                .len()
                .saturating_add(row_heights.len())
                .saturating_mul(48),
            MutationPatch::SheetDeleted { .. }
            | MutationPatch::SheetInvalidated { .. }
            | MutationPatch::RowInserted { .. }
            | MutationPatch::RowDeleted { .. }
            | MutationPatch::ColumnInserted { .. }
            | MutationPatch::ColumnDeleted { .. } => 96,
        })
        .sum::<usize>();
    std::mem::size_of::<MutationOutcome>()
        .saturating_add(patch_bytes)
        .saturating_add(2048)
}

fn estimated_cell_value_bytes(value: &CellValue) -> usize {
    match value {
        CellValue::Null => 8,
        CellValue::String(value) => value.len().saturating_mul(6).saturating_add(16),
        CellValue::Number(_) | CellValue::Boolean(_) => 32,
        CellValue::Formula {
            formula,
            cached_value,
            error,
        } => formula
            .len()
            .saturating_mul(6)
            .saturating_add(estimated_cell_value_bytes(cached_value))
            .saturating_add(
                error
                    .as_ref()
                    .map_or(0, |value| value.len().saturating_mul(6)),
            )
            .saturating_add(64),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_data::DocumentData;
    use crate::ops::patch_projector::status_mutation_outcome;
    use crate::projection_model::MutationLookupStatus;
    use crate::state::editor_state::EditorState;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, mpsc};
    use std::thread;
    use std::time::Duration;

    fn response() -> MutationOutcome {
        let state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "book.xlsx".to_string(),
                sheets: Vec::new(),
            },
            None,
        );
        status_mutation_outcome(&state)
    }

    #[test]
    fn replays_successful_mutations_once() {
        let coordinator = Arc::new(MutationReplayCoordinator::default());
        let calls = AtomicUsize::new(0);
        let first = run(&coordinator, 91, 0, "command", "set_cell", &(0, 0), || {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(response())
        })
        .expect("first mutation");
        let second = run(&coordinator, 91, 0, "command", "set_cell", &(0, 0), || {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(response())
        })
        .expect("replayed mutation");

        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(first.revision, second.revision);
        retire_document(&coordinator, 91);
    }

    #[test]
    fn first_and_replayed_results_share_the_same_replay_budget_projection() {
        use crate::projection_model::{MutationPatch, SheetLayoutSnapshot, SheetManifestSnapshot};

        let coordinator = Arc::new(MutationReplayCoordinator::default());
        let calls = AtomicUsize::new(0);
        let oversized_response = || {
            let mut outcome = response();
            outcome.patches = vec![MutationPatch::SheetsReplaced {
                start_index: 0,
                sheets: vec![SheetManifestSnapshot {
                    name: "x".repeat(MAX_REPLAY_BYTES),
                    extent: Default::default(),
                    layout: SheetLayoutSnapshot::default(),
                }],
            }];
            outcome
        };

        let first = run(&coordinator, 99, 0, "large", "replace", &(), || {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(oversized_response())
        })
        .expect("first mutation");
        let replayed = run(&coordinator, 99, 0, "large", "replace", &(), || {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(oversized_response())
        })
        .expect("replayed mutation");

        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert!(matches!(
            first.patches.as_slice(),
            [MutationPatch::ResyncRequired { reason }]
                if reason == "mutation response exceeded replay budget"
        ));
        assert!(matches!(
            replayed.patches.as_slice(),
            [MutationPatch::ResyncRequired { reason }]
                if reason == "mutation response exceeded replay budget"
        ));
        retire_document(&coordinator, 99);
    }

    #[test]
    fn concurrent_retries_share_one_execution() {
        let coordinator = Arc::new(MutationReplayCoordinator::default());
        let document_id = 92;
        let started = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let calls = Arc::new(AtomicUsize::new(0));

        let first = {
            let coordinator = Arc::clone(&coordinator);
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            let calls = Arc::clone(&calls);
            thread::spawn(move || {
                run(
                    &coordinator,
                    document_id,
                    0,
                    "shared",
                    "set_cell",
                    &(0, 0),
                    || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        started.wait();
                        release.wait();
                        Ok(response())
                    },
                )
            })
        };
        started.wait();
        let second = {
            let coordinator = Arc::clone(&coordinator);
            let calls = Arc::clone(&calls);
            thread::spawn(move || {
                run(
                    &coordinator,
                    document_id,
                    0,
                    "shared",
                    "set_cell",
                    &(0, 0),
                    || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(response())
                    },
                )
            })
        };

        thread::sleep(Duration::from_millis(20));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        release.wait();
        assert!(first.join().expect("first caller").is_ok());
        assert!(second.join().expect("retry caller").is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        retire_document(&coordinator, document_id);
    }

    #[test]
    fn unrelated_result_queries_do_not_wait_for_a_running_mutation() {
        let coordinator = Arc::new(MutationReplayCoordinator::default());
        let document_id = 93;
        let started = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let mutation = {
            let coordinator = Arc::clone(&coordinator);
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            thread::spawn(move || {
                run(
                    &coordinator,
                    document_id,
                    0,
                    "running",
                    "set_cell",
                    &(0, 0),
                    || {
                        started.wait();
                        release.wait();
                        Ok(response())
                    },
                )
            })
        };
        started.wait();

        let (sender, receiver) = mpsc::channel();
        let query_coordinator = Arc::clone(&coordinator);
        thread::spawn(move || {
            sender
                .send(get(&query_coordinator, 94, "unrelated"))
                .expect("query result")
        });
        let result = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("unrelated query should not block")
            .expect("query succeeds");
        assert_eq!(result.status, MutationLookupStatus::Missing);

        release.wait();
        assert!(mutation.join().expect("mutation caller").is_ok());
        retire_document(&coordinator, document_id);
    }

    #[test]
    fn matching_result_query_reports_pending_without_waiting() {
        let coordinator = Arc::new(MutationReplayCoordinator::default());
        let document_id = 95;
        let started = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let mutation = {
            let coordinator = Arc::clone(&coordinator);
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            thread::spawn(move || {
                run(
                    &coordinator,
                    document_id,
                    0,
                    "running",
                    "set_cell",
                    &(0, 0),
                    || {
                        started.wait();
                        release.wait();
                        Ok(response())
                    },
                )
            })
        };
        started.wait();

        let lookup = get(&coordinator, document_id, "running").expect("query result");
        assert_eq!(lookup.status, MutationLookupStatus::Pending);
        release.wait();
        assert!(mutation.join().expect("mutation caller").is_ok());
        let lookup = get(&coordinator, document_id, "running").expect("completed query result");
        assert_eq!(lookup.status, MutationLookupStatus::Completed);
        assert!(lookup.response.is_some());
        retire_document(&coordinator, document_id);
    }

    #[test]
    fn an_in_flight_command_id_rejects_a_different_payload() {
        let coordinator = Arc::new(MutationReplayCoordinator::default());
        let document_id = 96;
        let started = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let mutation = {
            let coordinator = Arc::clone(&coordinator);
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            thread::spawn(move || {
                run(
                    &coordinator,
                    document_id,
                    0,
                    "running",
                    "set_cell",
                    &(0, 0),
                    || {
                        started.wait();
                        release.wait();
                        Ok(response())
                    },
                )
            })
        };
        started.wait();

        let error = run(
            &coordinator,
            document_id,
            0,
            "running",
            "set_cell",
            &(1, 0),
            || Ok(response()),
        )
        .expect_err("different payload must be rejected");
        assert!(matches!(error, AppError::DocumentStateInvalid(_)));

        release.wait();
        assert!(mutation.join().expect("mutation caller").is_ok());
        retire_document(&coordinator, document_id);
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
        let coordinator = Arc::new(MutationReplayCoordinator::default());
        let mut reservations = Vec::new();
        for index in 0..MAX_IN_FLIGHT_MUTATIONS {
            let fingerprint = request_fingerprint(0, "set_cell", &index).expect("fingerprint");
            let reservation = reserve_with_coordinator(
                &coordinator,
                97,
                &format!("command-{index}"),
                fingerprint,
            )
            .expect("reservation");
            let ReservationResult::Execute(reservation) = reservation else {
                panic!("new command must reserve execution");
            };
            reservations.push(reservation);
        }

        let fingerprint = request_fingerprint(0, "set_cell", &999).expect("fingerprint");
        let error =
            match reserve_with_coordinator(&coordinator, 97, "one-command-too-many", fingerprint) {
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
        let coordinator = Arc::new(MutationReplayCoordinator::default());
        let fingerprint = request_fingerprint(0, "set_cell", &(0, 0)).expect("fingerprint");
        let reservation = reserve_with_coordinator(&coordinator, 98, "retiring", fingerprint)
            .expect("reservation");
        let ReservationResult::Execute(reservation) = reservation else {
            panic!("new command must reserve execution");
        };

        retire_document_with_coordinator(&coordinator, 98).expect("retire document");
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
