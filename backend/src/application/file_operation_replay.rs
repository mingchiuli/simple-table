use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use sha2::{Digest, Sha256};

use crate::error::AppError;
use crate::projection_model::{FileOperationKind, FileOperationReceipt};

const MAX_TERMINAL_FILE_OPERATIONS: usize = 128;
const MAX_IN_FLIGHT_FILE_OPERATIONS: usize = 16;
const MAX_OPERATION_ID_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FileOperationFingerprint([u8; 32]);

impl FileOperationFingerprint {
    pub(crate) fn open(
        token: &str,
        expected_document_id: Option<u64>,
        expected_revision: Option<u64>,
    ) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"open\0");
        hash_text(&mut digest, token);
        hash_optional_u64(&mut digest, expected_document_id);
        hash_optional_u64(&mut digest, expected_revision);
        Self(digest.finalize().into())
    }

    pub(crate) fn close(document_id: u64, revision: u64) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"close\0");
        digest.update(document_id.to_le_bytes());
        digest.update(revision.to_le_bytes());
        Self(digest.finalize().into())
    }
}

#[derive(Clone, Default)]
pub(crate) struct FileOperationReplayCoordinator {
    state: Arc<Mutex<FileOperationReplayState>>,
}

#[derive(Default)]
struct FileOperationReplayState {
    in_flight: HashMap<String, FileOperationFingerprint>,
    terminal: HashMap<String, TerminalFileOperation>,
    terminal_order: VecDeque<String>,
}

#[derive(Clone)]
struct TerminalFileOperation {
    fingerprint: FileOperationFingerprint,
    result: TerminalFileOperationResult,
}

#[derive(Clone)]
enum TerminalFileOperationResult {
    Completed,
    Failed(AppError),
}

pub(crate) enum FileOperationAdmission {
    Execute(FileOperationReservation),
    Pending,
    Completed,
    Failed(AppError),
}

pub(crate) struct FileOperationReservation {
    coordinator: FileOperationReplayCoordinator,
    operation_id: String,
    fingerprint: FileOperationFingerprint,
    finished: bool,
}

impl FileOperationReplayCoordinator {
    pub(crate) fn reserve(
        &self,
        operation_id: &str,
        fingerprint: FileOperationFingerprint,
    ) -> Result<FileOperationAdmission, AppError> {
        validate_operation_id(operation_id)?;
        let mut state = self.lock();
        if let Some(terminal) = state.terminal.get(operation_id) {
            ensure_same_fingerprint(terminal.fingerprint, fingerprint)?;
            return Ok(match &terminal.result {
                TerminalFileOperationResult::Completed => FileOperationAdmission::Completed,
                TerminalFileOperationResult::Failed(error) => {
                    FileOperationAdmission::Failed(error.clone())
                }
            });
        }
        if let Some(in_flight) = state.in_flight.get(operation_id) {
            ensure_same_fingerprint(*in_flight, fingerprint)?;
            return Ok(FileOperationAdmission::Pending);
        }
        if state.in_flight.len() >= MAX_IN_FLIGHT_FILE_OPERATIONS {
            return Err(AppError::ResourceLimitExceeded(format!(
                "at most {MAX_IN_FLIGHT_FILE_OPERATIONS} file operations may be in flight"
            )));
        }
        state
            .in_flight
            .insert(operation_id.to_string(), fingerprint);
        Ok(FileOperationAdmission::Execute(FileOperationReservation {
            coordinator: self.clone(),
            operation_id: operation_id.to_string(),
            fingerprint,
            finished: false,
        }))
    }

    fn lock(&self) -> MutexGuard<'_, FileOperationReplayState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl FileOperationReservation {
    pub(crate) fn complete(mut self, receipt: FileOperationReceipt) -> FileOperationReceipt {
        self.store_terminal(TerminalFileOperationResult::Completed);
        receipt
    }

    pub(crate) fn fail(mut self, error: AppError) -> AppError {
        self.store_terminal(TerminalFileOperationResult::Failed(error.clone()));
        error
    }

    fn store_terminal(&mut self, result: TerminalFileOperationResult) {
        let mut state = self.coordinator.lock();
        state.in_flight.remove(&self.operation_id);
        while state.terminal_order.len() >= MAX_TERMINAL_FILE_OPERATIONS {
            if let Some(expired) = state.terminal_order.pop_front() {
                state.terminal.remove(&expired);
            }
        }
        state.terminal_order.push_back(self.operation_id.clone());
        state.terminal.insert(
            self.operation_id.clone(),
            TerminalFileOperation {
                fingerprint: self.fingerprint,
                result,
            },
        );
        self.finished = true;
    }
}

impl Drop for FileOperationReservation {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        self.store_terminal(TerminalFileOperationResult::Failed(AppError::Internal(
            "file operation ended before reaching a terminal state".to_string(),
        )));
    }
}

pub(crate) fn completed_operation_error(kind: FileOperationKind) -> AppError {
    AppError::DocumentStateInvalid(format!(
        "{} operation already completed; use a new operationId",
        operation_name(kind)
    ))
}

pub(crate) fn pending_operation_error(kind: FileOperationKind) -> AppError {
    AppError::DocumentStateInvalid(format!(
        "{} operation is still pending",
        operation_name(kind)
    ))
}

fn operation_name(kind: FileOperationKind) -> &'static str {
    match kind {
        FileOperationKind::Open => "open",
        FileOperationKind::Close => "close",
    }
}

fn validate_operation_id(operation_id: &str) -> Result<(), AppError> {
    if operation_id.is_empty() || operation_id.len() > MAX_OPERATION_ID_BYTES {
        return Err(AppError::DocumentStateInvalid(
            "file operationId must contain between 1 and 128 bytes".to_string(),
        ));
    }
    Ok(())
}

fn ensure_same_fingerprint(
    current: FileOperationFingerprint,
    requested: FileOperationFingerprint,
) -> Result<(), AppError> {
    if current == requested {
        return Ok(());
    }
    Err(AppError::DocumentStateInvalid(
        "file operationId was reused with a different payload".to_string(),
    ))
}

fn hash_text(digest: &mut Sha256, value: &str) {
    digest.update((value.len() as u64).to_le_bytes());
    digest.update(value.as_bytes());
}

fn hash_optional_u64(digest: &mut Sha256, value: Option<u64>) {
    match value {
        Some(value) => {
            digest.update([1]);
            digest.update(value.to_le_bytes());
        }
        None => digest.update([0]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(revision: u64) -> FileOperationReceipt {
        FileOperationReceipt {
            kind: FileOperationKind::Close,
            document_id: 7,
            revision,
            path: "/tmp/book.xlsx".to_string(),
            file_name: "book.xlsx".to_string(),
        }
    }

    #[test]
    fn completed_operations_are_not_admitted_twice() {
        let coordinator = FileOperationReplayCoordinator::default();
        let fingerprint = FileOperationFingerprint::close(7, 3);
        let FileOperationAdmission::Execute(reservation) = coordinator
            .reserve("operation-1", fingerprint)
            .expect("reserve")
        else {
            panic!("first request must execute");
        };
        assert_eq!(reservation.complete(receipt(4)), receipt(4));
        assert!(matches!(
            coordinator.reserve("operation-1", fingerprint),
            Ok(FileOperationAdmission::Completed)
        ));
    }

    #[test]
    fn dropped_reservations_become_terminal_failures() {
        let coordinator = FileOperationReplayCoordinator::default();
        let fingerprint = FileOperationFingerprint::open("token", None, None);
        let reservation = match coordinator
            .reserve("operation-2", fingerprint)
            .expect("reserve")
        {
            FileOperationAdmission::Execute(reservation) => reservation,
            _ => panic!("first request must execute"),
        };
        drop(reservation);
        assert!(matches!(
            coordinator.reserve("operation-2", fingerprint),
            Ok(FileOperationAdmission::Failed(AppError::Internal(message)))
                if message == "file operation ended before reaching a terminal state"
        ));
    }

    #[test]
    fn failed_operations_replay_the_original_error() {
        let coordinator = FileOperationReplayCoordinator::default();
        let fingerprint = FileOperationFingerprint::close(7, 3);
        let reservation = match coordinator
            .reserve("operation-failed", fingerprint)
            .expect("reserve")
        {
            FileOperationAdmission::Execute(reservation) => reservation,
            _ => panic!("first request must execute"),
        };
        reservation.fail(AppError::DocumentStateInvalid(
            "revision changed".to_string(),
        ));
        assert!(matches!(
            coordinator.reserve("operation-failed", fingerprint),
            Ok(FileOperationAdmission::Failed(AppError::DocumentStateInvalid(message)))
                if message == "revision changed"
        ));
    }

    #[test]
    fn operation_ids_cannot_be_reused_for_other_payloads() {
        let coordinator = FileOperationReplayCoordinator::default();
        let first = FileOperationFingerprint::close(7, 3);
        let second = FileOperationFingerprint::close(7, 4);
        let _reservation = match coordinator.reserve("operation-3", first).expect("reserve") {
            FileOperationAdmission::Execute(reservation) => reservation,
            _ => panic!("first request must execute"),
        };
        assert!(matches!(
            coordinator.reserve("operation-3", second),
            Err(AppError::DocumentStateInvalid(_))
        ));
    }
}
