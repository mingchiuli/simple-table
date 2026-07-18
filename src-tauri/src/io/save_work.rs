use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::error::AppError;

const MAX_CONCURRENT_SAVE_WORK: usize = 1;
const MAX_SAVE_SOURCE_BYTES: usize = 256 * 1024 * 1024;

#[derive(Default)]
struct SaveWorkState {
    active_documents: HashSet<u64>,
    active_jobs: usize,
    reserved_source_bytes: usize,
}

impl SaveWorkState {
    fn begin(&mut self, document_id: u64, source_bytes: usize) -> Result<(), AppError> {
        if source_bytes > MAX_SAVE_SOURCE_BYTES {
            return Err(AppError::ResourceLimitExceeded(format!(
                "save source requires an estimated {source_bytes} bytes; the maximum is {MAX_SAVE_SOURCE_BYTES} bytes"
            )));
        }
        if self.active_documents.contains(&document_id) {
            return Err(AppError::DocumentStateInvalid(
                "save or export preparation is already in progress for this document".to_string(),
            ));
        }
        if self.active_jobs >= MAX_CONCURRENT_SAVE_WORK {
            return Err(AppError::ResourceLimitExceeded(
                "save and export work is at its concurrency limit".to_string(),
            ));
        }

        self.active_documents.insert(document_id);
        self.active_jobs += 1;
        self.reserved_source_bytes = self.reserved_source_bytes.saturating_add(source_bytes);
        Ok(())
    }

    fn finish(&mut self, document_id: u64, source_bytes: usize) {
        if !self.active_documents.remove(&document_id) {
            return;
        }
        self.active_jobs = self.active_jobs.saturating_sub(1);
        self.reserved_source_bytes = self.reserved_source_bytes.saturating_sub(source_bytes);
    }
}

#[derive(Clone, Default)]
pub struct SaveWorkCoordinator {
    state: Arc<Mutex<SaveWorkState>>,
}

pub(crate) struct SaveWorkReservation {
    coordinator: SaveWorkCoordinator,
    document_id: u64,
    source_bytes: usize,
    active: bool,
}

impl Drop for SaveWorkReservation {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        if let Ok(mut state) = self.coordinator.state.lock() {
            state.finish(self.document_id, self.source_bytes);
            self.active = false;
        }
    }
}

impl SaveWorkCoordinator {
    pub(crate) fn reserve(
        &self,
        document_id: u64,
        estimated_source_bytes: usize,
    ) -> Result<SaveWorkReservation, AppError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AppError::poisoned_lock("save work coordinator"))?;
        state.begin(document_id, estimated_source_bytes)?;
        drop(state);
        Ok(SaveWorkReservation {
            coordinator: self.clone(),
            document_id,
            source_bytes: estimated_source_bytes,
            active: true,
        })
    }

    #[cfg(test)]
    pub(crate) fn is_same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_work_is_exclusive_and_released() {
        let mut state = SaveWorkState::default();
        state.begin(1, 1024).expect("first save work");

        assert!(matches!(
            state.begin(1, 1024),
            Err(AppError::DocumentStateInvalid(_))
        ));
        assert!(matches!(
            state.begin(2, 1024),
            Err(AppError::ResourceLimitExceeded(_))
        ));

        state.finish(1, 1024);
        assert!(state.begin(2, 2048).is_ok());
        assert_eq!(state.active_jobs, 1);
        assert_eq!(state.reserved_source_bytes, 2048);
    }

    #[test]
    fn save_work_rejects_an_oversized_source_before_admission() {
        let mut state = SaveWorkState::default();
        let error = state
            .begin(1, MAX_SAVE_SOURCE_BYTES + 1)
            .expect_err("oversized source");

        assert!(matches!(error, AppError::ResourceLimitExceeded(_)));
        assert_eq!(state.active_jobs, 0);
    }

    #[test]
    fn save_work_coordinators_have_isolated_admission_state() {
        let first = SaveWorkCoordinator::default();
        let second = SaveWorkCoordinator::default();
        let reservation = first.reserve(1, 1024).expect("first reservation");

        assert!(first.reserve(2, 1024).is_err());
        assert!(second.reserve(2, 1024).is_ok());

        drop(reservation);
        assert!(first.reserve(3, 1024).is_ok());
    }
}
