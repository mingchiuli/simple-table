use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::application::document_work_budget_port::{DocumentWorkBudgetPort, DocumentWorkLease};
use crate::error::AppError;
use crate::resource_limits::{MAX_DOCUMENT_WORKING_SET_BYTES, MAX_SAVE_SOURCE_BYTES};

#[derive(Clone, Copy)]
enum WorkKind {
    Preparation,
    Save { document_id: u64 },
}

#[derive(Clone, Copy)]
struct WorkReservationState {
    kind: WorkKind,
    work_bytes: usize,
}

#[derive(Default)]
struct DocumentWorkState {
    next_reservation_id: u64,
    active_document_bytes: usize,
    reservations: HashMap<u64, WorkReservationState>,
    active_save_documents: HashSet<u64>,
}

impl DocumentWorkState {
    fn begin(
        &mut self,
        kind: WorkKind,
        active_document_bytes: usize,
        work_bytes: usize,
    ) -> Result<u64, AppError> {
        if let WorkKind::Save { document_id } = kind
            && self.active_save_documents.contains(&document_id)
        {
            return Err(AppError::DocumentStateInvalid(
                "save or export preparation is already in progress for this document".to_string(),
            ));
        }
        if matches!(kind, WorkKind::Save { .. }) && !self.active_save_documents.is_empty() {
            return Err(AppError::ResourceLimitExceeded(
                "save and export work is at its concurrency limit".to_string(),
            ));
        }

        let active_document_bytes = self.active_document_bytes.max(active_document_bytes);
        self.ensure_peak(
            active_document_bytes,
            self.reserved_work_bytes(),
            work_bytes,
        )?;
        self.next_reservation_id = self.next_reservation_id.checked_add(1).ok_or_else(|| {
            AppError::Internal("document work reservation identifiers are exhausted".to_string())
        })?;
        let reservation_id = self.next_reservation_id;
        self.active_document_bytes = active_document_bytes;
        self.reservations
            .insert(reservation_id, WorkReservationState { kind, work_bytes });
        if let WorkKind::Save { document_id } = kind {
            self.active_save_documents.insert(document_id);
        }
        Ok(reservation_id)
    }

    fn set_work_bytes(&mut self, reservation_id: u64, work_bytes: usize) -> Result<(), AppError> {
        let previous = self
            .reservations
            .get(&reservation_id)
            .copied()
            .ok_or_else(|| {
                AppError::DocumentStateInvalid(
                    "document work reservation is no longer active".to_string(),
                )
            })?;
        let other_work_bytes = self
            .reserved_work_bytes()
            .saturating_sub(previous.work_bytes);
        self.ensure_peak(self.active_document_bytes, other_work_bytes, work_bytes)?;
        if let Some(reservation) = self.reservations.get_mut(&reservation_id) {
            reservation.work_bytes = work_bytes;
        }
        Ok(())
    }

    fn finish(&mut self, reservation_id: u64) {
        let Some(reservation) = self.reservations.remove(&reservation_id) else {
            return;
        };
        if let WorkKind::Save { document_id } = reservation.kind {
            self.active_save_documents.remove(&document_id);
        }
        if self.reservations.is_empty() {
            self.active_document_bytes = 0;
        }
    }

    fn reserved_work_bytes(&self) -> usize {
        self.reservations
            .values()
            .fold(0usize, |total, reservation| {
                total.saturating_add(reservation.work_bytes)
            })
    }

    fn ensure_peak(
        &self,
        active_document_bytes: usize,
        existing_work_bytes: usize,
        requested_work_bytes: usize,
    ) -> Result<(), AppError> {
        let peak_bytes = active_document_bytes
            .saturating_add(existing_work_bytes)
            .saturating_add(requested_work_bytes);
        if peak_bytes > MAX_DOCUMENT_WORKING_SET_BYTES {
            return Err(AppError::ResourceLimitExceeded(format!(
                "document work requires an estimated peak of {peak_bytes} bytes, maximum is {MAX_DOCUMENT_WORKING_SET_BYTES} bytes"
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Default)]
pub(crate) struct DocumentWorkBudgetAdapter {
    state: Arc<Mutex<DocumentWorkState>>,
}

struct DocumentWorkReservation {
    coordinator: DocumentWorkBudgetAdapter,
    reservation_id: u64,
    active: bool,
}

impl Drop for DocumentWorkReservation {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        if let Ok(mut state) = self.coordinator.state.lock() {
            state.finish(self.reservation_id);
            self.active = false;
        }
    }
}

impl DocumentWorkLease for DocumentWorkReservation {
    fn set_work_bytes(&mut self, work_bytes: usize) -> Result<(), AppError> {
        let mut state = self
            .coordinator
            .state
            .lock()
            .map_err(|_| AppError::poisoned_lock("document work coordinator"))?;
        state.set_work_bytes(self.reservation_id, work_bytes)
    }
}

impl DocumentWorkBudgetPort for DocumentWorkBudgetAdapter {
    fn reserve_preparation(
        &self,
        active_document_bytes: usize,
        estimated_work_bytes: usize,
    ) -> Result<Box<dyn DocumentWorkLease>, AppError> {
        self.reserve(
            WorkKind::Preparation,
            active_document_bytes,
            estimated_work_bytes,
        )
    }

    fn reserve_save(
        &self,
        document_id: u64,
        active_document_bytes: usize,
        estimated_source_bytes: usize,
    ) -> Result<Box<dyn DocumentWorkLease>, AppError> {
        if estimated_source_bytes > MAX_SAVE_SOURCE_BYTES {
            return Err(AppError::ResourceLimitExceeded(format!(
                "save source requires an estimated {estimated_source_bytes} bytes; the maximum is {MAX_SAVE_SOURCE_BYTES} bytes"
            )));
        }
        self.reserve(
            WorkKind::Save { document_id },
            active_document_bytes,
            estimated_source_bytes,
        )
    }
}

impl DocumentWorkBudgetAdapter {
    fn reserve(
        &self,
        kind: WorkKind,
        active_document_bytes: usize,
        work_bytes: usize,
    ) -> Result<Box<dyn DocumentWorkLease>, AppError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AppError::poisoned_lock("document work coordinator"))?;
        let reservation_id = state.begin(kind, active_document_bytes, work_bytes)?;
        drop(state);
        Ok(Box::new(DocumentWorkReservation {
            coordinator: self.clone(),
            reservation_id,
            active: true,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource_limits::{MAX_GENERATED_FILE_BYTES, MAX_PREPARED_DOCUMENT_BYTES};

    #[test]
    fn save_work_is_exclusive_and_released() {
        let budget = DocumentWorkBudgetAdapter::default();
        let reservation = budget.reserve_save(1, 1024, 1024).expect("first save work");

        assert!(matches!(
            budget.reserve_save(1, 1024, 1024),
            Err(AppError::DocumentStateInvalid(_))
        ));
        assert!(matches!(
            budget.reserve_save(2, 1024, 1024),
            Err(AppError::ResourceLimitExceeded(_))
        ));

        drop(reservation);
        assert!(budget.reserve_save(1, 1024, 1024).is_ok());
    }

    #[test]
    fn preparation_and_save_share_one_peak_budget() {
        let budget = DocumentWorkBudgetAdapter::default();
        let _preparation = budget
            .reserve_preparation(MAX_SAVE_SOURCE_BYTES, MAX_PREPARED_DOCUMENT_BYTES)
            .expect("preparation reservation");
        let mut save = budget
            .reserve_save(7, MAX_SAVE_SOURCE_BYTES, MAX_SAVE_SOURCE_BYTES)
            .expect("initial save reservation");

        let expanded_save = MAX_SAVE_SOURCE_BYTES
            .saturating_add(MAX_GENERATED_FILE_BYTES)
            .saturating_add(MAX_PREPARED_DOCUMENT_BYTES);
        assert!(matches!(
            save.set_work_bytes(expanded_save),
            Err(AppError::ResourceLimitExceeded(_))
        ));
    }

    #[test]
    fn work_budget_is_released_by_raii() {
        let budget = DocumentWorkBudgetAdapter::default();
        let mut reservation = budget.reserve_save(1, 0, 1024).expect("save reservation");
        reservation
            .set_work_bytes(MAX_DOCUMENT_WORKING_SET_BYTES)
            .expect("consume budget");
        assert!(budget.reserve_preparation(0, 1).is_err());

        drop(reservation);
        assert!(budget.reserve_preparation(0, 1).is_ok());
    }
}
