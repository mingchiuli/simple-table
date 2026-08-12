use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use web_time::{Duration, Instant};

use crate::adapters::search_index_backend::{
    SearchIndexBuildOutcome, SearchIndexCellUpdate, WRITER_ARENA_BYTES,
    build_sheet_index_with_cancel,
};
use crate::adapters::search_index_registry::SearchIndexStamp;
use crate::adapters::search_index_scheduler::{
    CellIndexUpdate, IndexScheduler, IndexSchedulerState, SearchSchedulerStats, SheetPending,
    update_pending_stats,
};
use crate::application::search_ports::SearchDocumentSourcePort;
use crate::domain::SearchCellText;

const INDEX_DEBOUNCE: Duration = Duration::from_millis(300);
pub(super) const MAX_INDEXABLE_SHEET_BYTES: usize = 12 * 1024 * 1024;
pub(super) const MAX_BUILDING_INDEX_BYTES: usize = 64 * 1024 * 1024;

pub(super) fn create_index_scheduler(
    source: Arc<dyn SearchDocumentSourcePort>,
) -> (Arc<IndexScheduler>, Vec<thread::JoinHandle<()>>) {
    let scheduler = Arc::new(IndexScheduler {
        state: Mutex::new(IndexSchedulerState::default()),
        indexes: Arc::new(Mutex::new(Default::default())),
        source,
        wake: Condvar::new(),
        workers_available: AtomicBool::new(false),
        shutdown: AtomicBool::new(false),
    });
    let worker_count = thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(2)
        .clamp(2, 4);
    let mut workers = Vec::with_capacity(worker_count);
    for worker_index in 0..worker_count {
        let worker_scheduler = Arc::clone(&scheduler);
        match thread::Builder::new()
            .name(format!("simple-table-indexer-{worker_index}"))
            .spawn(move || index_worker(&worker_scheduler))
        {
            Ok(worker) => {
                workers.push(worker);
                scheduler.workers_available.store(true, Ordering::Release);
            }
            Err(error) => eprintln!("Failed to spawn search index worker thread: {error}"),
        }
    }
    (scheduler, workers)
}

fn index_worker(scheduler: &Arc<IndexScheduler>) {
    while let Some(((_, sheet_index), pending, _active_sheet)) = drain_pending_job(scheduler) {
        process_pending_sheet(scheduler, sheet_index, pending);
    }
}

pub(super) fn process_pending_sheet(
    scheduler: &Arc<IndexScheduler>,
    sheet_index: usize,
    pending: SheetPending,
) {
    if let Some(rebuild) = pending.rebuild {
        record_scheduler_event(scheduler, |stats| {
            stats.rebuild_jobs = stats.rebuild_jobs.saturating_add(1);
        });
        let latest_stamp = pending
            .incremental
            .values()
            .map(|update| update.stamp)
            .chain(std::iter::once(rebuild.stamp))
            .max()
            .unwrap_or(rebuild.stamp);
        rebuild_sheet_with_budget(scheduler, pending.document_id, sheet_index, latest_stamp);
        return;
    }

    if !pending.incremental.is_empty() {
        record_scheduler_event(scheduler, |stats| {
            stats.incremental_jobs = stats.incremental_jobs.saturating_add(1);
        });
        let latest_stamp = pending
            .incremental
            .values()
            .map(|update| update.stamp)
            .max();
        let Some(latest_stamp) = latest_stamp else {
            return;
        };
        let updates: Vec<CellIndexUpdate> = pending.incremental.into_values().collect();
        if !run_incremental(scheduler, pending.document_id, sheet_index, &updates) {
            record_scheduler_event(scheduler, |stats| {
                stats.incremental_fallback_rebuilds =
                    stats.incremental_fallback_rebuilds.saturating_add(1);
                stats.rebuild_jobs = stats.rebuild_jobs.saturating_add(1);
            });
            rebuild_sheet_with_budget(scheduler, pending.document_id, sheet_index, latest_stamp);
        }
    }
}

struct ActiveSheetReservation {
    scheduler: Arc<IndexScheduler>,
    key: (u64, usize),
}

impl Drop for ActiveSheetReservation {
    fn drop(&mut self) {
        let mut state = self
            .scheduler
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.active_sheets.remove(&self.key);
        self.scheduler.wake.notify_all();
    }
}

fn drain_pending_job(
    scheduler: &Arc<IndexScheduler>,
) -> Option<((u64, usize), SheetPending, ActiveSheetReservation)> {
    let mut state = scheduler
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    loop {
        if scheduler.shutdown.load(Ordering::Acquire) {
            return None;
        }
        while !has_ready_job(&state) {
            state = scheduler
                .wake
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if scheduler.shutdown.load(Ordering::Acquire) {
                return None;
            }
        }

        let deadline = Instant::now() + INDEX_DEBOUNCE;
        loop {
            let now = Instant::now();
            if now >= deadline || state.pending.is_empty() {
                break;
            }
            let wait = deadline - now;
            let wait_result = scheduler.wake.wait_timeout(state, wait);
            let (next_state, timeout) = match wait_result {
                Ok(result) => result,
                Err(poisoned) => poisoned.into_inner(),
            };
            state = next_state;
            if timeout.timed_out() {
                break;
            }
        }

        let Some((key, pending)) = take_ready_job(&mut state) else {
            continue;
        };
        if has_ready_job(&state) {
            scheduler.wake.notify_one();
        }
        state.stats.drained_batches = state.stats.drained_batches.saturating_add(1);
        return Some((
            key,
            pending,
            ActiveSheetReservation {
                scheduler: Arc::clone(scheduler),
                key,
            },
        ));
    }
}

fn has_ready_job(state: &IndexSchedulerState) -> bool {
    state
        .pending
        .keys()
        .any(|key| !state.active_sheets.contains(key))
}

fn take_ready_job(state: &mut IndexSchedulerState) -> Option<((u64, usize), SheetPending)> {
    let key = state
        .pending
        .keys()
        .find(|key| !state.active_sheets.contains(key))
        .copied()?;
    let pending = state.pending.remove(&key)?;
    state.active_sheets.insert(key);
    state.pending_updates = state
        .pending_updates
        .saturating_sub(pending.incremental.len());
    state.pending_bytes = state
        .pending_bytes
        .saturating_sub(pending.incremental_bytes);
    update_pending_stats(state);
    Some((key, pending))
}

fn record_scheduler_event(
    scheduler: &Arc<IndexScheduler>,
    update: impl FnOnce(&mut SearchSchedulerStats),
) {
    if let Ok(mut state) = scheduler.state.lock() {
        update(&mut state.stats);
    }
}

pub(super) struct IndexBuildReservation {
    scheduler: Arc<IndexScheduler>,
    bytes: usize,
}

impl Drop for IndexBuildReservation {
    fn drop(&mut self) {
        if let Ok(mut state) = self.scheduler.state.lock() {
            state.building_jobs = state.building_jobs.saturating_sub(1);
            state.building_bytes = state.building_bytes.saturating_sub(self.bytes);
            update_building_stats(&mut state);
            self.scheduler.wake.notify_all();
        }
    }
}

pub(super) fn index_build_reservation_bytes(source_bytes: usize) -> Option<usize> {
    if source_bytes > MAX_INDEXABLE_SHEET_BYTES {
        return None;
    }
    Some(
        WRITER_ARENA_BYTES
            .saturating_add(source_bytes.saturating_mul(4))
            .min(MAX_BUILDING_INDEX_BYTES),
    )
}

pub(super) fn reserve_index_build(
    scheduler: &Arc<IndexScheduler>,
    bytes: usize,
) -> Option<IndexBuildReservation> {
    if bytes == 0 || bytes > MAX_BUILDING_INDEX_BYTES {
        return None;
    }
    let mut state = scheduler.state.lock().ok()?;
    while state.building_bytes.saturating_add(bytes) > MAX_BUILDING_INDEX_BYTES {
        if scheduler.shutdown.load(Ordering::Acquire) {
            return None;
        }
        state = scheduler
            .wake
            .wait(state)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
    state.building_jobs = state.building_jobs.saturating_add(1);
    state.building_bytes = state.building_bytes.saturating_add(bytes);
    update_building_stats(&mut state);
    Some(IndexBuildReservation {
        scheduler: Arc::clone(scheduler),
        bytes,
    })
}

fn update_building_stats(state: &mut IndexSchedulerState) {
    state.stats.building_jobs = state.building_jobs;
    state.stats.building_bytes = state.building_bytes;
    state.stats.peak_building_jobs = state.stats.peak_building_jobs.max(state.building_jobs);
    state.stats.peak_building_bytes = state.stats.peak_building_bytes.max(state.building_bytes);
}

fn rebuild_sheet_with_budget(
    scheduler: &Arc<IndexScheduler>,
    document_id: u64,
    sheet_index: usize,
    stamp: SearchIndexStamp,
) {
    let Some(source_bytes) =
        search_sheet_source_estimated_bytes(scheduler, document_id, sheet_index, stamp)
    else {
        return;
    };
    let Some(reservation_bytes) = index_build_reservation_bytes(source_bytes) else {
        record_scheduler_event(scheduler, |stats| {
            stats.skipped_oversized_rebuilds = stats.skipped_oversized_rebuilds.saturating_add(1);
        });
        return;
    };
    let Some(_reservation) = reserve_index_build(scheduler, reservation_bytes) else {
        return;
    };
    let Some(search_text) = snapshot_sheet_search_text(scheduler, document_id, sheet_index, stamp)
    else {
        return;
    };
    run_rebuild(scheduler, document_id, sheet_index, stamp, search_text);
}

fn search_sheet_source_estimated_bytes(
    scheduler: &Arc<IndexScheduler>,
    document_id: u64,
    sheet_index: usize,
    stamp: SearchIndexStamp,
) -> Option<usize> {
    if !search_stamp_is_current(scheduler, document_id, sheet_index, stamp) {
        return None;
    }
    scheduler
        .source
        .document_snapshot(document_id, Some(stamp.source_revision))
        .ok()
        .flatten()?
        .sheets
        .get(sheet_index)
        .map(|sheet| sheet.estimated_source_bytes)
}

pub(super) fn run_rebuild(
    scheduler: &Arc<IndexScheduler>,
    document_id: u64,
    sheet_index: usize,
    stamp: SearchIndexStamp,
    search_text: Arc<[SearchCellText]>,
) {
    if !search_stamp_is_current(scheduler, document_id, sheet_index, stamp) {
        return;
    }

    let built_index = match build_sheet_index_with_cancel(&search_text, || {
        search_stamp_is_current(scheduler, document_id, sheet_index, stamp)
    }) {
        Ok(SearchIndexBuildOutcome::Built(index)) => index,
        Ok(SearchIndexBuildOutcome::Cancelled) => return,
        Err(error) => {
            eprintln!("Search index rebuild failed: {error}");
            record_scheduler_event(scheduler, |stats| {
                stats.failed_rebuilds = stats.failed_rebuilds.saturating_add(1);
            });
            return;
        }
    };

    let sheet_count = scheduler
        .source
        .document_snapshot(document_id, Some(stamp.source_revision))
        .ok()
        .flatten()
        .map(|snapshot| snapshot.sheets.len());
    let Some(sheet_count) = sheet_count else {
        return;
    };
    let retired = {
        let Ok(mut indexes) = scheduler.indexes.lock() else {
            return;
        };
        let store = indexes.document_mut(document_id);
        if store.sheet_stamp(document_id, sheet_index) != stamp {
            return;
        }
        let mut retired =
            store.install_sheet_index(document_id, sheet_index, stamp, Some(built_index));
        retired.append(store.truncate(sheet_count));
        retired
    };
    drop(retired);
}

fn search_stamp_is_current(
    scheduler: &Arc<IndexScheduler>,
    document_id: u64,
    sheet_index: usize,
    stamp: SearchIndexStamp,
) -> bool {
    let source_is_current = scheduler
        .source
        .document_snapshot(document_id, Some(stamp.source_revision))
        .ok()
        .flatten()
        .is_some();
    source_is_current
        && scheduler
            .indexes
            .lock()
            .ok()
            .and_then(|indexes| {
                indexes
                    .document(document_id)
                    .map(|store| store.sheet_stamp(document_id, sheet_index) == stamp)
            })
            .unwrap_or(false)
}

pub(super) fn snapshot_sheet_search_text(
    scheduler: &Arc<IndexScheduler>,
    document_id: u64,
    sheet_index: usize,
    stamp: SearchIndexStamp,
) -> Option<Arc<[SearchCellText]>> {
    if !search_stamp_is_current(scheduler, document_id, sheet_index, stamp) {
        return None;
    }
    scheduler
        .source
        .sheet_text_snapshot(document_id, stamp.source_revision, sheet_index)
        .ok()
        .flatten()
}

fn run_incremental(
    scheduler: &Arc<IndexScheduler>,
    document_id: u64,
    sheet_index: usize,
    ops: &[CellIndexUpdate],
) -> bool {
    let Some(latest_stamp) = ops.iter().map(|op| op.stamp).max() else {
        return true;
    };
    if ops.iter().any(|op| {
        op.stamp.document_id != latest_stamp.document_id
            || op.stamp.generation != latest_stamp.generation
            || op.stamp > latest_stamp
    }) {
        return false;
    }
    let Some(index) = scheduler.indexes.lock().ok().and_then(|indexes| {
        indexes
            .document(document_id)
            .and_then(|store| store.incremental_index(document_id, sheet_index, latest_stamp))
    }) else {
        return false;
    };
    if let Err(error) = index.apply_updates(ops.iter().map(|op| SearchIndexCellUpdate {
        row: op.row,
        col: op.col,
        search_text: &op.search_text,
        display_text: &op.display_text,
    })) {
        eprintln!("incremental search index update failed: {error}");
        return false;
    }

    if !search_stamp_is_current(scheduler, document_id, sheet_index, latest_stamp) {
        return false;
    }
    let retired = scheduler.indexes.lock().ok().and_then(|mut indexes| {
        indexes.document(document_id)?;
        Some(indexes.document_mut(document_id).mark_sheet_fresh(
            document_id,
            sheet_index,
            latest_stamp,
        ))
    });
    drop(retired);

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::search_index_scheduler::{IndexJob, merge_job};

    fn update(document_id: u64, sheet_index: usize, revision: u64) -> IndexJob {
        IndexJob::UpdateCell {
            document_id,
            sheet_index,
            stamp: SearchIndexStamp {
                document_id,
                generation: 1,
                source_revision: revision,
                revision,
            },
            row: 0,
            col: 0,
            search_text: revision.to_string(),
            display_text: revision.to_string(),
        }
    }

    #[test]
    fn same_sheet_batches_wait_while_other_sheets_remain_parallel() {
        let mut state = IndexSchedulerState::default();
        merge_job(&mut state, update(7, 2, 1));

        let (active_key, _) = take_ready_job(&mut state).expect("first sheet batch");
        assert_eq!(active_key, (7, 2));
        assert!(state.active_sheets.contains(&active_key));

        merge_job(&mut state, update(7, 2, 2));
        merge_job(&mut state, update(7, 3, 1));

        let (parallel_key, _) = take_ready_job(&mut state).expect("parallel sheet batch");
        assert_eq!(parallel_key, (7, 3));
        assert!(take_ready_job(&mut state).is_none());

        state.active_sheets.remove(&active_key);
        let (next_key, next) = take_ready_job(&mut state).expect("next same-sheet batch");
        assert_eq!(next_key, active_key);
        assert_eq!(
            next.incremental
                .get(&(0, 0))
                .expect("coalesced update")
                .stamp
                .revision,
            2
        );
    }
}
