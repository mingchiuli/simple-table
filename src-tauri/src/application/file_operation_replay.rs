use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use sha2::{Digest, Sha256};

use crate::error::AppError;
use crate::projection_model::{FileOperationKind, FileOperationLookup, FileOperationReceipt};

const MAX_COMPLETED_FILE_OPERATIONS: usize = 128;
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

    pub(crate) fn save(path: &str, document_id: u64, revision: u64) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"save\0");
        hash_text(&mut digest, path);
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
    completed: HashMap<String, CompletedFileOperation>,
    completed_order: VecDeque<String>,
}

#[derive(Clone)]
struct CompletedFileOperation {
    fingerprint: FileOperationFingerprint,
    receipt: FileOperationReceipt,
}

pub(crate) enum FileOperationAdmission {
    Execute(FileOperationReservation),
    Pending,
    Completed,
}

pub(crate) struct FileOperationReservation {
    coordinator: FileOperationReplayCoordinator,
    operation_id: String,
    fingerprint: FileOperationFingerprint,
    finished: bool,
}

impl FileOperationReplayCoordinator {
    #[cfg(test)]
    pub(crate) fn is_same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }

    pub(crate) fn reserve(
        &self,
        operation_id: &str,
        fingerprint: FileOperationFingerprint,
    ) -> Result<FileOperationAdmission, AppError> {
        validate_operation_id(operation_id)?;
        let mut state = self.lock();
        if let Some(completed) = state.completed.get(operation_id) {
            ensure_same_fingerprint(completed.fingerprint, fingerprint)?;
            return Ok(FileOperationAdmission::Completed);
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

    pub(crate) fn get(&self, operation_id: &str) -> Result<FileOperationLookup, AppError> {
        validate_operation_id(operation_id)?;
        let state = self.lock();
        if state.in_flight.contains_key(operation_id) {
            return Ok(FileOperationLookup::pending());
        }
        Ok(state
            .completed
            .get(operation_id)
            .map_or_else(FileOperationLookup::missing, |completed| {
                FileOperationLookup::completed(completed.receipt.clone())
            }))
    }

    fn lock(&self) -> MutexGuard<'_, FileOperationReplayState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl FileOperationReservation {
    pub(crate) fn finish(mut self, receipt: FileOperationReceipt) -> FileOperationReceipt {
        let mut state = self.coordinator.lock();
        state.in_flight.remove(&self.operation_id);
        while state.completed_order.len() >= MAX_COMPLETED_FILE_OPERATIONS {
            if let Some(expired) = state.completed_order.pop_front() {
                state.completed.remove(&expired);
            }
        }
        state.completed_order.push_back(self.operation_id.clone());
        state.completed.insert(
            self.operation_id.clone(),
            CompletedFileOperation {
                fingerprint: self.fingerprint,
                receipt: receipt.clone(),
            },
        );
        self.finished = true;
        receipt
    }
}

impl Drop for FileOperationReservation {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let mut state = self.coordinator.lock();
        if state.in_flight.get(&self.operation_id) == Some(&self.fingerprint) {
            state.in_flight.remove(&self.operation_id);
        }
    }
}

pub(crate) fn completed_operation_error(kind: FileOperationKind) -> AppError {
    let operation = match kind {
        FileOperationKind::Open => "open",
        FileOperationKind::Save => "save",
    };
    AppError::DocumentStateInvalid(format!(
        "{operation} operation already completed; query its operationId for the receipt"
    ))
}

pub(crate) fn pending_operation_error(kind: FileOperationKind) -> AppError {
    let operation = match kind {
        FileOperationKind::Open => "open",
        FileOperationKind::Save => "save",
    };
    AppError::DocumentStateInvalid(format!(
        "{operation} operation is still pending; query its operationId for the receipt"
    ))
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
            kind: FileOperationKind::Save,
            document_id: 7,
            revision,
            path: "/tmp/book.xlsx".to_string(),
            file_name: "book.xlsx".to_string(),
        }
    }

    #[test]
    fn completed_operations_are_not_admitted_twice() {
        let coordinator = FileOperationReplayCoordinator::default();
        let fingerprint = FileOperationFingerprint::save("/tmp/book.xlsx", 7, 3);
        let FileOperationAdmission::Execute(reservation) = coordinator
            .reserve("operation-1", fingerprint)
            .expect("reserve")
        else {
            panic!("first request must execute");
        };
        reservation.finish(receipt(4));

        assert!(matches!(
            coordinator.reserve("operation-1", fingerprint),
            Ok(FileOperationAdmission::Completed)
        ));
        assert_eq!(
            coordinator.get("operation-1").expect("lookup").receipt,
            Some(receipt(4))
        );
    }

    #[test]
    fn dropped_reservations_release_in_flight_admission() {
        let coordinator = FileOperationReplayCoordinator::default();
        let fingerprint = FileOperationFingerprint::open("token", None, None);
        let reservation = match coordinator
            .reserve("operation-2", fingerprint)
            .expect("reserve")
        {
            FileOperationAdmission::Execute(reservation) => reservation,
            _ => panic!("first request must execute"),
        };
        assert_eq!(
            coordinator.get("operation-2").expect("pending").status,
            crate::projection_model::FileOperationLookupStatus::Pending
        );
        drop(reservation);
        assert_eq!(
            coordinator.get("operation-2").expect("missing").status,
            crate::projection_model::FileOperationLookupStatus::Missing
        );
    }

    #[test]
    fn operation_ids_cannot_be_reused_for_other_payloads() {
        let coordinator = FileOperationReplayCoordinator::default();
        let first = FileOperationFingerprint::save("/tmp/first.xlsx", 7, 3);
        let second = FileOperationFingerprint::save("/tmp/second.xlsx", 7, 3);
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
