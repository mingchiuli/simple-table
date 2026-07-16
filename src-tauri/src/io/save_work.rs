use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

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

pub(crate) struct SaveWorkReservation {
    document_id: u64,
    source_bytes: usize,
    active: bool,
}

impl Drop for SaveWorkReservation {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        if let Ok(mut state) = state().lock() {
            state.finish(self.document_id, self.source_bytes);
            self.active = false;
        }
    }
}

pub(crate) fn reserve(
    document_id: u64,
    estimated_source_bytes: usize,
) -> Result<SaveWorkReservation, AppError> {
    let mut state = state()
        .lock()
        .map_err(|_| AppError::poisoned_lock("save work coordinator"))?;
    state.begin(document_id, estimated_source_bytes)?;
    Ok(SaveWorkReservation {
        document_id,
        source_bytes: estimated_source_bytes,
        active: true,
    })
}

fn state() -> &'static Mutex<SaveWorkState> {
    static SAVE_WORK: OnceLock<Mutex<SaveWorkState>> = OnceLock::new();
    SAVE_WORK.get_or_init(|| Mutex::new(SaveWorkState::default()))
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
}
