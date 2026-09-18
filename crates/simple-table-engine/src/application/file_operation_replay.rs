use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::application::replay::{Fingerprint, FingerprintWriter, TerminalCache, entry_overhead};
use crate::error::AppError;
use crate::snapshot::{FileOperationKind, FileOperationReceipt};

const MAX_TERMINAL_FILE_OPERATIONS: usize = 128;
const MAX_TERMINAL_FILE_OPERATION_BYTES: usize = 512 * 1024;
const MAX_IN_FLIGHT_FILE_OPERATIONS: usize = 16;
const MAX_OPERATION_ID_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FileOperationFingerprint(Fingerprint);

impl FileOperationFingerprint {
    pub(crate) fn open(
        token: &str,
        expected_document_id: Option<u64>,
        expected_revision: Option<u64>,
    ) -> Self {
        let mut writer = FingerprintWriter::default();
        writer.write_bytes(b"open\0");
        writer.write_text(token);
        writer.write_optional_u64(expected_document_id);
        writer.write_optional_u64(expected_revision);
        Self(writer.finish())
    }

    pub(crate) fn close(document_id: u64, revision: u64) -> Self {
        let mut writer = FingerprintWriter::default();
        writer.write_bytes(b"close\0");
        writer.write_u64(document_id);
        writer.write_u64(revision);
        Self(writer.finish())
    }
}

#[derive(Clone)]
pub(crate) struct FileOperationReplayCoordinator {
    state: Arc<Mutex<FileOperationReplayState>>,
}

impl Default for FileOperationReplayCoordinator {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(FileOperationReplayState::default())),
        }
    }
}

struct FileOperationReplayState {
    in_flight: HashMap<String, FileOperationFingerprint>,
    terminal: TerminalCache<String, TerminalFileOperationResult>,
}

impl Default for FileOperationReplayState {
    fn default() -> Self {
        Self {
            in_flight: HashMap::new(),
            terminal: TerminalCache::new(
                MAX_TERMINAL_FILE_OPERATIONS,
                MAX_TERMINAL_FILE_OPERATION_BYTES,
            ),
        }
    }
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
        if let Some(terminal) = state.terminal.find(|key| key == operation_id) {
            ensure_same_fingerprint(terminal.fingerprint, fingerprint)?;
            return Ok(match &terminal.value {
                TerminalFileOperationResult::Completed => FileOperationAdmission::Completed,
                TerminalFileOperationResult::Failed(error) => {
                    FileOperationAdmission::Failed(error.clone())
                }
            });
        }
        if let Some(in_flight) = state.in_flight.get(operation_id) {
            ensure_same_fingerprint(in_flight.0, fingerprint)?;
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
        let bytes = terminal_entry_bytes(&self.operation_id, &result);
        state
            .terminal
            .insert(self.operation_id.clone(), self.fingerprint.0, result, bytes);
        self.finished = true;
    }
}

fn terminal_entry_bytes(operation_id: &str, result: &TerminalFileOperationResult) -> usize {
    let payload_bytes = match result {
        TerminalFileOperationResult::Completed => 0,
        TerminalFileOperationResult::Failed(error) => error.to_string().len(),
    };
    operation_id
        .len()
        .saturating_add(payload_bytes)
        .saturating_add(entry_overhead::<String, TerminalFileOperationResult>())
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
    current: Fingerprint,
    requested: FileOperationFingerprint,
) -> Result<(), AppError> {
    if current == requested.0 {
        return Ok(());
    }
    Err(AppError::DocumentStateInvalid(
        "file operationId was reused with a different payload".to_string(),
    ))
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
