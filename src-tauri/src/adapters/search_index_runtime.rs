use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tantivy::{Term, doc};

use crate::adapters::search_index_scheduler::{
    CellIndexUpdate, IndexJob, IndexScheduler, IndexSchedulerState, MAX_BUILDING_INDEX_BYTES,
    MAX_INDEXABLE_SHEET_BYTES, MAX_PENDING_INDEX_BYTES, MAX_PENDING_INDEX_BYTES_PER_SHEET,
    MAX_PENDING_INDEX_SHEETS, MAX_PENDING_INDEX_UPDATES_PER_SHEET, RebuildIndexUpdate,
    SearchSchedulerStats, SheetPending,
};
use crate::adapters::search_index_store::{
    MAX_RESIDENT_SEARCH_INDEXES, SearchIndexStamp, WRITER_ARENA_BYTES,
    build_sheet_index_with_cancel, search_position,
};
use crate::application::search_ports::SearchDocumentSourcePort;
#[cfg(test)]
use crate::domain::CellValue;
use crate::domain::{
    SearchCellIndexUpdate, SearchCellText, SearchIndexWork, SearchOutcome, SearchScope,
};

const INDEX_DEBOUNCE: Duration = Duration::from_millis(300);
const SEARCH_SCAN_RESERVATION_BYTES: usize = 24 * 1024 * 1024;

pub(crate) struct SearchIndexRuntime {
    pub(crate) scheduler: Arc<IndexScheduler>,
    scan_work: Arc<Mutex<usize>>,
    workers: Mutex<Vec<thread::JoinHandle<()>>>,
}

impl Drop for SearchIndexRuntime {
    fn drop(&mut self) {
        self.scheduler.shutdown.store(true, Ordering::Release);
        self.scheduler
            .workers_available
            .store(false, Ordering::Release);
        self.scheduler.wake.notify_all();
        let workers = self
            .workers
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for worker in workers.drain(..) {
            if worker.thread().id() != thread::current().id() {
                let _ = worker.join();
            }
        }
    }
}

impl SearchIndexRuntime {
    pub(crate) fn new(source: Arc<dyn SearchDocumentSourcePort>) -> Arc<Self> {
        let (scheduler, workers) = create_index_scheduler(source);
        Arc::new(Self {
            scheduler,
            scan_work: Arc::new(Mutex::new(0)),
            workers: Mutex::new(workers),
        })
    }

    #[cfg(test)]
    fn from_scheduler(scheduler: Arc<IndexScheduler>) -> Arc<Self> {
        Arc::new(Self {
            scheduler,
            scan_work: Arc::new(Mutex::new(0)),
            workers: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn search(
        &self,
        document_id: u64,
        base_revision: u64,
        query: &str,
        scope: SearchScope,
        current_sheet_index: Option<usize>,
    ) -> Result<SearchOutcome, crate::error::AppError> {
        crate::adapters::search_query_engine::execute_search(
            self.scheduler.source.as_ref(),
            document_id,
            base_revision,
            query,
            scope,
            current_sheet_index,
            |sheet_index| self.fresh_sheet_index(document_id, base_revision, sheet_index),
            || self.reserve_scan_work(),
            |sheet_index| self.rebuild_sheet_index(document_id, sheet_index),
        )
    }

    fn fresh_sheet_index(
        &self,
        document_id: u64,
        source_revision: u64,
        sheet_index: usize,
    ) -> Option<Arc<crate::adapters::search_index_store::SearchSheetIndex>> {
        let mut indexes = self.scheduler.indexes.lock().ok()?;
        indexes
            .synchronize_revision(document_id, source_revision)?
            .fresh_sheet_index(sheet_index)
    }

    fn reserve_scan_work(&self) -> Result<SearchScanReservation, crate::error::AppError> {
        let mut active_bytes = self
            .scan_work
            .lock()
            .map_err(|_| crate::error::AppError::poisoned_lock("search scan work"))?;
        if active_bytes.saturating_add(SEARCH_SCAN_RESERVATION_BYTES)
            > SEARCH_SCAN_RESERVATION_BYTES
        {
            return Err(crate::error::AppError::ResourceLimitExceeded(
                "another search scan is already using the fallback memory budget".to_string(),
            ));
        }
        *active_bytes += SEARCH_SCAN_RESERVATION_BYTES;
        drop(active_bytes);
        Ok(SearchScanReservation {
            active_bytes: Arc::clone(&self.scan_work),
        })
    }

    pub(crate) fn stats(&self) -> SearchSchedulerStats {
        self.scheduler
            .state
            .lock()
            .map(|state| state.stats.clone())
            .unwrap_or_default()
    }

    pub(crate) fn rebuild_all_sheets_index(&self, document_id: u64) {
        let Some(snapshot) = self
            .scheduler
            .source
            .document_snapshot(document_id, None)
            .ok()
            .flatten()
        else {
            return;
        };
        let jobs = self.prepare_rebuild_all(document_id, snapshot.revision, snapshot.sheets.len());

        for (sheet_index, stamp) in jobs {
            self.enqueue(IndexJob::Rebuild {
                document_id,
                sheet_index,
                stamp,
            });
        }
    }

    fn prepare_rebuild_all(
        &self,
        document_id: u64,
        source_revision: u64,
        sheet_count: usize,
    ) -> Vec<(usize, SearchIndexStamp)> {
        let Ok(mut indexes) = self.scheduler.indexes.lock() else {
            return Vec::new();
        };
        let store = indexes.document_mut(document_id);
        if source_revision < store.source_revision() {
            return Vec::new();
        }
        store.set_source_revision(source_revision);
        store.mark_stale(document_id);
        (0..sheet_count.min(MAX_RESIDENT_SEARCH_INDEXES))
            .map(|sheet_index| (sheet_index, store.sheet_stamp(document_id, sheet_index)))
            .collect()
    }

    pub(crate) fn rebuild_sheet_index(&self, document_id: u64, sheet_index: usize) {
        let Some(snapshot) = self
            .scheduler
            .source
            .document_snapshot(document_id, None)
            .ok()
            .flatten()
        else {
            return;
        };
        if sheet_index >= snapshot.sheets.len() {
            return;
        }
        let stamp = {
            let Ok(mut indexes) = self.scheduler.indexes.lock() else {
                return;
            };
            let Some(store) = indexes.synchronize_revision(document_id, snapshot.revision) else {
                return;
            };
            store.sheet_stamp(document_id, sheet_index)
        };
        self.enqueue_rebuild(document_id, sheet_index, stamp);
    }

    fn enqueue_cell_update(
        &self,
        document_id: u64,
        stamp: SearchIndexStamp,
        update: SearchCellIndexUpdate,
    ) {
        self.enqueue(IndexJob::UpdateCell {
            document_id,
            sheet_index: update.sheet_index,
            stamp,
            row: update.row,
            col: update.col,
            search_text: update.search_text,
            display_text: update.display_text,
        });
    }

    fn enqueue_rebuild(&self, document_id: u64, sheet_index: usize, stamp: SearchIndexStamp) {
        self.enqueue(IndexJob::Rebuild {
            document_id,
            sheet_index,
            stamp,
        });
    }

    pub(crate) fn schedule_work(
        &self,
        document_id: u64,
        source_revision: u64,
        work: SearchIndexWork,
    ) {
        let Some(snapshot) = self
            .scheduler
            .source
            .document_snapshot(document_id, Some(source_revision))
            .ok()
            .flatten()
        else {
            return;
        };
        match work {
            SearchIndexWork::None => {
                if let Ok(mut indexes) = self.scheduler.indexes.lock() {
                    let store = indexes.document_mut(document_id);
                    if source_revision >= store.source_revision() {
                        store.set_source_revision(source_revision);
                    }
                }
            }
            SearchIndexWork::UpdateCells(updates) => {
                let stamps = {
                    let Ok(mut indexes) = self.scheduler.indexes.lock() else {
                        return;
                    };
                    let store = indexes.document_mut(document_id);
                    if source_revision < store.source_revision() {
                        return;
                    }
                    if source_revision > store.source_revision().saturating_add(1) {
                        store.set_source_revision(source_revision);
                        store.mark_stale(document_id);
                    } else {
                        store.set_source_revision(source_revision);
                        let mut sheet_indexes = updates
                            .iter()
                            .map(|update| update.sheet_index)
                            .collect::<Vec<_>>();
                        sheet_indexes.sort_unstable();
                        sheet_indexes.dedup();
                        for sheet_index in sheet_indexes {
                            store.mark_sheet_stale(sheet_index);
                        }
                    }
                    updates
                        .iter()
                        .map(|update| {
                            (
                                update.sheet_index,
                                store.sheet_stamp(document_id, update.sheet_index),
                            )
                        })
                        .collect::<HashMap<_, _>>()
                };
                for update in updates {
                    let Some(stamp) = stamps.get(&update.sheet_index).copied() else {
                        continue;
                    };
                    self.enqueue_cell_update(document_id, stamp, update);
                }
            }
            SearchIndexWork::RebuildAll => {
                let jobs =
                    self.prepare_rebuild_all(document_id, source_revision, snapshot.sheets.len());
                for (sheet_index, stamp) in jobs {
                    self.enqueue_rebuild(document_id, sheet_index, stamp);
                }
            }
        }
    }

    pub(crate) fn cancel_document_jobs(&self, document_id: u64) {
        if let Ok(mut state) = self.scheduler.state.lock() {
            let removed: Vec<_> = state
                .pending
                .extract_if(|(pending_document_id, _), _| *pending_document_id == document_id)
                .map(|(_, pending)| pending)
                .collect();
            let canceled = removed.len();
            if canceled > 0 {
                state.pending_updates = state.pending_updates.saturating_sub(
                    removed
                        .iter()
                        .map(|pending| pending.incremental.len())
                        .sum(),
                );
                state.pending_bytes = state.pending_bytes.saturating_sub(
                    removed
                        .iter()
                        .map(|pending| pending.incremental_bytes)
                        .sum(),
                );
                state.stats.canceled_batches =
                    state.stats.canceled_batches.saturating_add(canceled as u64);
                update_pending_stats(&mut state);
                self.scheduler.wake.notify_all();
            }
        }
        if let Ok(mut indexes) = self.scheduler.indexes.lock() {
            drop(indexes.remove(document_id));
        }
    }

    fn enqueue(&self, job: IndexJob) {
        if let Ok(mut state) = self.scheduler.state.lock() {
            if !self.scheduler.workers_available.load(Ordering::Acquire) {
                state.stats.dropped_jobs_no_workers =
                    state.stats.dropped_jobs_no_workers.saturating_add(1);
                return;
            }
            state.stats.queued_jobs = state.stats.queued_jobs.saturating_add(1);
            merge_job(&mut state, job);
            self.scheduler.wake.notify_one();
        }
    }
}

struct SearchScanReservation {
    active_bytes: Arc<Mutex<usize>>,
}

impl Drop for SearchScanReservation {
    fn drop(&mut self) {
        if let Ok(mut active_bytes) = self.active_bytes.lock() {
            *active_bytes = active_bytes.saturating_sub(SEARCH_SCAN_RESERVATION_BYTES);
        }
    }
}

fn create_index_scheduler(
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

fn merge_job(state: &mut IndexSchedulerState, job: IndexJob) {
    let document_id = job.document_id();
    let sheet_index = job.sheet_index();
    let key = (document_id, sheet_index);
    if !state.pending.contains_key(&key) && state.pending.len() >= MAX_PENDING_INDEX_SHEETS {
        state.stats.dropped_jobs_at_capacity =
            state.stats.dropped_jobs_at_capacity.saturating_add(1);
        update_pending_stats(state);
        return;
    }

    let total_bytes_before = state.pending_bytes;
    let total_updates_before = state.pending_updates;
    let mut coalesced_to_rebuild = false;
    let (previous_entry_bytes, next_entry_bytes, previous_entry_updates, next_entry_updates) = {
        let entry = state.pending.entry(key).or_insert_with(|| SheetPending {
            document_id,
            rebuild: None,
            incremental: HashMap::new(),
            incremental_bytes: 0,
        });
        let previous_entry_bytes = entry.incremental_bytes;
        let previous_entry_updates = entry.incremental.len();

        match job {
            IndexJob::Rebuild { stamp, .. } => {
                let latest_seen = latest_pending_stamp(entry);
                if latest_seen.is_none_or(|latest| stamp >= latest) {
                    entry.rebuild = Some(RebuildIndexUpdate { stamp });
                    retain_updates_after(entry, stamp);
                } else if entry.rebuild.is_none() {
                    entry.rebuild = Some(RebuildIndexUpdate { stamp });
                    retain_updates_after(entry, stamp);
                }
            }
            IndexJob::UpdateCell {
                stamp,
                row,
                col,
                search_text,
                display_text,
                ..
            } => {
                if entry
                    .rebuild
                    .as_ref()
                    .is_some_and(|rebuild| stamp <= rebuild.stamp)
                {
                    return;
                }
                if entry
                    .incremental
                    .get(&(row, col))
                    .is_some_and(|existing| stamp < existing.stamp)
                {
                    return;
                }

                let update = CellIndexUpdate {
                    stamp,
                    row,
                    col,
                    search_text,
                    display_text,
                };
                let previous_update_bytes = entry
                    .incremental
                    .get(&(row, col))
                    .map(cell_index_update_bytes)
                    .unwrap_or(0);
                let next_update_bytes = cell_index_update_bytes(&update);
                let next_sheet_updates = entry.incremental.len()
                    + usize::from(!entry.incremental.contains_key(&(row, col)));
                let next_sheet_bytes = entry
                    .incremental_bytes
                    .saturating_sub(previous_update_bytes)
                    .saturating_add(next_update_bytes);
                let next_total_bytes = total_bytes_before
                    .saturating_sub(previous_entry_bytes)
                    .saturating_add(next_sheet_bytes);

                if next_sheet_updates > MAX_PENDING_INDEX_UPDATES_PER_SHEET
                    || next_sheet_bytes > MAX_PENDING_INDEX_BYTES_PER_SHEET
                    || next_total_bytes > MAX_PENDING_INDEX_BYTES
                {
                    let latest_stamp = latest_pending_stamp(entry)
                        .into_iter()
                        .chain(std::iter::once(stamp))
                        .max()
                        .unwrap_or(stamp);
                    entry.rebuild = Some(RebuildIndexUpdate {
                        stamp: latest_stamp,
                    });
                    entry.incremental.clear();
                    entry.incremental_bytes = 0;
                    coalesced_to_rebuild = true;
                } else {
                    entry.incremental.insert((row, col), update);
                    entry.incremental_bytes = next_sheet_bytes;
                }
            }
        }

        (
            previous_entry_bytes,
            entry.incremental_bytes,
            previous_entry_updates,
            entry.incremental.len(),
        )
    };

    state.pending_bytes = total_bytes_before
        .saturating_sub(previous_entry_bytes)
        .saturating_add(next_entry_bytes);
    state.pending_updates = total_updates_before
        .saturating_sub(previous_entry_updates)
        .saturating_add(next_entry_updates);
    if coalesced_to_rebuild {
        state.stats.coalesced_to_rebuilds = state.stats.coalesced_to_rebuilds.saturating_add(1);
    }
    update_pending_stats(state);
}

fn latest_pending_stamp(entry: &SheetPending) -> Option<SearchIndexStamp> {
    entry
        .rebuild
        .as_ref()
        .map(|rebuild| rebuild.stamp)
        .into_iter()
        .chain(entry.incremental.values().map(|update| update.stamp).max())
        .max()
}

fn retain_updates_after(entry: &mut SheetPending, stamp: SearchIndexStamp) {
    entry.incremental.retain(|_, update| update.stamp > stamp);
    entry.incremental_bytes = entry
        .incremental
        .values()
        .map(cell_index_update_bytes)
        .sum();
}

fn cell_index_update_bytes(update: &CellIndexUpdate) -> usize {
    std::mem::size_of::<((usize, usize), CellIndexUpdate)>()
        .saturating_add(update.search_text.capacity())
        .saturating_add(update.display_text.capacity())
}

fn update_pending_stats(state: &mut IndexSchedulerState) {
    state.stats.pending_sheets = state.pending.len();
    state.stats.pending_updates = state.pending_updates;
    state.stats.pending_bytes = state.pending_bytes;
}

fn index_worker(scheduler: &Arc<IndexScheduler>) {
    while let Some(((_, sheet_index), pending)) = drain_pending_job(scheduler) {
        process_pending_sheet(scheduler, sheet_index, pending);
    }
}

fn process_pending_sheet(
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

fn drain_pending_job(scheduler: &Arc<IndexScheduler>) -> Option<((u64, usize), SheetPending)> {
    let mut state = scheduler
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    loop {
        if scheduler.shutdown.load(Ordering::Acquire) {
            return None;
        }
        while state.pending.is_empty() {
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

        let Some(key) = state.pending.keys().next().copied() else {
            continue;
        };
        let Some(pending) = state.pending.remove(&key) else {
            continue;
        };
        state.pending_updates = state
            .pending_updates
            .saturating_sub(pending.incremental.len());
        state.pending_bytes = state
            .pending_bytes
            .saturating_sub(pending.incremental_bytes);
        update_pending_stats(&mut state);
        if !state.pending.is_empty() {
            scheduler.wake.notify_one();
        }
        state.stats.drained_batches = state.stats.drained_batches.saturating_add(1);
        return Some((key, pending));
    }
}

fn record_scheduler_event(
    scheduler: &Arc<IndexScheduler>,
    update: impl FnOnce(&mut SearchSchedulerStats),
) {
    if let Ok(mut state) = scheduler.state.lock() {
        update(&mut state.stats);
    }
}

struct IndexBuildReservation {
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

fn index_build_reservation_bytes(source_bytes: usize) -> Option<usize> {
    if source_bytes > MAX_INDEXABLE_SHEET_BYTES {
        return None;
    }
    Some(
        WRITER_ARENA_BYTES
            .saturating_add(source_bytes.saturating_mul(4))
            .min(MAX_BUILDING_INDEX_BYTES),
    )
}

fn reserve_index_build(
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

#[allow(dead_code)]
pub(crate) fn search_scheduler_stats(search: &SearchIndexRuntime) -> SearchSchedulerStats {
    search.stats()
}

fn run_rebuild(
    scheduler: &Arc<IndexScheduler>,
    document_id: u64,
    sheet_index: usize,
    stamp: SearchIndexStamp,
    search_text: Arc<[SearchCellText]>,
) {
    if !search_stamp_is_current(scheduler, document_id, sheet_index, stamp) {
        return;
    }

    let built_index = build_sheet_index_with_cancel(&search_text, || {
        search_stamp_is_current(scheduler, document_id, sheet_index, stamp)
    });

    let sheet_count = scheduler
        .source
        .document_snapshot(document_id, Some(stamp.source_revision))
        .ok()
        .flatten()
        .map(|snapshot| snapshot.sheets.len());
    let Some(sheet_count) = sheet_count else {
        drop(built_index);
        return;
    };
    let retired = {
        let Ok(mut indexes) = scheduler.indexes.lock() else {
            drop(built_index);
            return;
        };
        let store = indexes.document_mut(document_id);
        if store.sheet_stamp(document_id, sheet_index) != stamp {
            drop(built_index);
            return;
        }
        let mut retired = store.install_sheet_index(document_id, sheet_index, stamp, built_index);
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

fn snapshot_sheet_search_text(
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
    let Some(writer_handle) = scheduler.indexes.lock().ok().and_then(|indexes| {
        indexes
            .document(document_id)
            .and_then(|store| store.writer_handle(document_id, sheet_index, latest_stamp))
    }) else {
        return false;
    };
    let mut writer = match writer_handle.writer.lock() {
        Ok(writer) => writer,
        Err(_) => return false,
    };

    for op in ops {
        let cell_id = format!("{}:{}", op.row, op.col);
        writer.delete_term(Term::from_field_text(writer_handle.cell_id_field, &cell_id));
        if !op.search_text.is_empty()
            && let Err(error) = writer.add_document(doc!(
                writer_handle.text_field => op.search_text.clone(),
                writer_handle.literal_field => op.search_text.to_lowercase(),
                writer_handle.display_field => op.display_text.clone(),
                writer_handle.row_field => op.row as u64,
                writer_handle.col_field => op.col as u64,
                writer_handle.position_field => search_position(op.row, op.col),
                writer_handle.cell_id_field => cell_id,
            ))
        {
            eprintln!("incremental add_document failed: {error:?}");
            return false;
        }
    }

    if let Err(error) = writer.commit() {
        eprintln!("incremental commit failed: {error:?}");
        return false;
    }
    drop(writer);

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
    use crate::adapters::search_document_source_adapter::RepositorySearchDocumentSource;
    use crate::adapters::search_index_store::SearchIndexRegistry;
    use crate::document_data::{CellFormat, RichMetadata};
    use crate::document_data::{DocumentData, DocumentSheet};
    use crate::domain::CellNumber;
    use crate::domain::EditorCommand;
    use crate::domain::SearchScope;
    use crate::error::AppError;
    use crate::state::editor_state::EditorState;
    use crate::state::state::ActiveDocumentRepository;

    struct TestContext {
        documents: ActiveDocumentRepository,
        document_id: u64,
        search: Arc<SearchIndexRuntime>,
    }

    fn s(value: &str) -> CellValue {
        CellValue::String(value.to_string())
    }

    fn context_for_sheet(sheet: DocumentSheet) -> TestContext {
        let editor = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "test.xlsx".to_string(),
                sheets: vec![sheet],
            },
            None,
        );
        let document_id = editor.document_id();
        let documents = ActiveDocumentRepository::default();
        documents.replace_active_for_test(editor);
        let source = Arc::new(RepositorySearchDocumentSource::new(documents.clone()));
        let search = isolated_search_adapter(source, true);
        TestContext {
            documents,
            document_id,
            search,
        }
    }

    fn context_for_rows(rows: Vec<Vec<CellValue>>) -> TestContext {
        context_for_sheet(DocumentSheet {
            name: "Test".to_string(),
            rows,
            ..Default::default()
        })
    }

    fn isolated_search_adapter(
        source: Arc<dyn SearchDocumentSourcePort>,
        workers_available: bool,
    ) -> Arc<SearchIndexRuntime> {
        SearchIndexRuntime::from_scheduler(Arc::new(IndexScheduler {
            state: Mutex::new(IndexSchedulerState::default()),
            indexes: Arc::new(Mutex::new(SearchIndexRegistry::default())),
            source,
            wake: Condvar::new(),
            workers_available: AtomicBool::new(workers_available),
            shutdown: AtomicBool::new(false),
        }))
    }

    fn active_revision(context: &TestContext) -> u64 {
        context
            .documents
            .read_handle(context.document_id)
            .expect("document")
            .read()
            .expect("state")
            .revision()
    }

    fn rebuild_sheet_now(context: &TestContext) {
        let snapshot = context
            .search
            .scheduler
            .source
            .document_snapshot(context.document_id, None)
            .expect("snapshot")
            .expect("active document");
        let (_, stamp) = context
            .search
            .prepare_rebuild_all(
                context.document_id,
                snapshot.revision,
                snapshot.sheets.len(),
            )
            .into_iter()
            .next()
            .expect("sheet rebuild");
        let search_text =
            snapshot_sheet_search_text(&context.search.scheduler, context.document_id, 0, stamp)
                .expect("search text");
        run_rebuild(
            &context.search.scheduler,
            context.document_id,
            0,
            stamp,
            search_text,
        );
    }

    fn search_rows(context: &TestContext, query: &str) -> Vec<(usize, usize)> {
        let mut rows = context
            .search
            .search(
                context.document_id,
                active_revision(context),
                query,
                SearchScope::CurrentSheet,
                Some(0),
            )
            .expect("search")
            .results
            .into_iter()
            .map(|result| (result.row, result.col))
            .collect::<Vec<_>>();
        rows.sort_unstable();
        rows
    }

    fn search_values(context: &TestContext, query: &str) -> Vec<String> {
        context
            .search
            .search(
                context.document_id,
                active_revision(context),
                query,
                SearchScope::CurrentSheet,
                Some(0),
            )
            .expect("search")
            .results
            .into_iter()
            .map(|result| result.value)
            .collect()
    }

    fn test_stamp(document_id: u64, source_revision: u64, revision: u64) -> SearchIndexStamp {
        SearchIndexStamp {
            document_id,
            generation: 1,
            source_revision,
            revision,
        }
    }

    #[test]
    fn dropping_the_runtime_stops_and_joins_workers() {
        let context = context_for_rows(vec![vec![s("value")]]);
        let source = Arc::new(RepositorySearchDocumentSource::new(
            context.documents.clone(),
        ));
        let runtime = SearchIndexRuntime::new(source);
        let scheduler = Arc::clone(&runtime.scheduler);

        assert!(!runtime.workers.lock().expect("workers").is_empty());
        drop(runtime);

        assert!(scheduler.shutdown.load(Ordering::Acquire));
        assert!(!scheduler.workers_available.load(Ordering::Acquire));
        assert_eq!(Arc::strong_count(&scheduler), 1);
    }

    #[test]
    fn search_scan_reservations_are_isolated_per_adapter() {
        let first = context_for_rows(vec![vec![s("first")]]);
        let second = context_for_rows(vec![vec![s("second")]]);
        let reservation = first.search.reserve_scan_work().expect("reservation");

        assert!(matches!(
            first.search.reserve_scan_work(),
            Err(AppError::ResourceLimitExceeded(_))
        ));
        assert!(second.search.reserve_scan_work().is_ok());
        drop(reservation);
        assert!(first.search.reserve_scan_work().is_ok());
    }

    #[test]
    fn index_build_reservations_are_bounded_and_released() {
        let context = context_for_rows(vec![vec![s("value")]]);
        let reservation = reserve_index_build(&context.search.scheduler, MAX_BUILDING_INDEX_BYTES)
            .expect("build reservation");
        assert_eq!(
            context
                .search
                .scheduler
                .state
                .lock()
                .unwrap()
                .building_bytes,
            MAX_BUILDING_INDEX_BYTES
        );

        drop(reservation);

        assert_eq!(
            context
                .search
                .scheduler
                .state
                .lock()
                .unwrap()
                .building_bytes,
            0
        );
        assert!(index_build_reservation_bytes(MAX_INDEXABLE_SHEET_BYTES).is_some());
        assert!(index_build_reservation_bytes(MAX_INDEXABLE_SHEET_BYTES + 1).is_none());
    }

    #[test]
    fn stale_document_revision_is_rejected_by_the_source_port() {
        let context = context_for_rows(vec![vec![s("old")]]);
        let revision = active_revision(&context);
        context
            .documents
            .read_handle(context.document_id)
            .unwrap()
            .write()
            .unwrap()
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "new".to_string(),
            })
            .unwrap();

        let error = context
            .search
            .search(
                context.document_id,
                revision,
                "new",
                SearchScope::CurrentSheet,
                Some(0),
            )
            .expect_err("stale revision");

        assert!(matches!(error, AppError::DocumentStateInvalid(_)));
    }

    #[test]
    fn rebuilding_and_searching_use_adapter_owned_indexes() {
        let context = context_for_rows(vec![
            vec![s("apple"), s("banana")],
            vec![s("cherry"), s("durian")],
        ]);
        rebuild_sheet_now(&context);

        assert_eq!(search_rows(&context, "apple"), vec![(0, 0)]);
        assert_eq!(search_rows(&context, "durian"), vec![(1, 1)]);
        assert!(
            context
                .search
                .fresh_sheet_index(context.document_id, active_revision(&context), 0)
                .is_some()
        );
    }

    #[test]
    fn revision_change_invalidates_index_before_work_is_scheduled() {
        let context = context_for_rows(vec![vec![s("old")]]);
        rebuild_sheet_now(&context);
        context
            .documents
            .read_handle(context.document_id)
            .unwrap()
            .write()
            .unwrap()
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "new".to_string(),
            })
            .unwrap();

        assert!(search_rows(&context, "old").is_empty());
        assert_eq!(search_rows(&context, "new"), vec![(0, 0)]);
    }

    #[test]
    fn incremental_update_refreshes_the_adapter_index() {
        let context = context_for_rows(vec![vec![s("apple"), s("banana")]]);
        rebuild_sheet_now(&context);
        context
            .documents
            .read_handle(context.document_id)
            .unwrap()
            .write()
            .unwrap()
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "orange".to_string(),
            })
            .unwrap();
        let revision = active_revision(&context);
        context.search.schedule_work(
            context.document_id,
            revision,
            SearchIndexWork::UpdateCells(vec![SearchCellIndexUpdate {
                sheet_index: 0,
                row: 0,
                col: 0,
                search_text: "orange".to_string(),
                display_text: "orange".to_string(),
            }]),
        );
        let pending = context
            .search
            .scheduler
            .state
            .lock()
            .unwrap()
            .pending
            .remove(&(context.document_id, 0))
            .expect("pending update");
        process_pending_sheet(&context.search.scheduler, 0, pending);

        assert!(search_rows(&context, "apple").is_empty());
        assert_eq!(search_rows(&context, "orange"), vec![(0, 0)]);
        assert_eq!(search_rows(&context, "banana"), vec![(0, 1)]);
    }

    #[test]
    fn scan_and_index_return_the_same_formatted_display_value() {
        let context = context_for_sheet(DocumentSheet {
            name: "Test".to_string(),
            rows: vec![vec![CellValue::Number(CellNumber::from_f64(0.4).unwrap())]],
            rich: RichMetadata {
                cell_formats: HashMap::from([(
                    "A1".to_string(),
                    CellFormat {
                        number_format: Some("0%".to_string()),
                        style_id: None,
                    },
                )]),
                ..Default::default()
            },
            ..Default::default()
        });

        assert_eq!(search_values(&context, "0.4"), vec!["40%"]);
        rebuild_sheet_now(&context);
        assert_eq!(search_values(&context, "0.4"), vec!["40%"]);
    }

    #[test]
    fn pending_jobs_coalesce_and_rebuild_supersedes_updates() {
        let context = context_for_rows(vec![vec![s("value")]]);
        let first = test_stamp(context.document_id, 1, 1);
        let second = test_stamp(context.document_id, 2, 2);
        let mut state = IndexSchedulerState::default();
        merge_job(
            &mut state,
            IndexJob::UpdateCell {
                document_id: context.document_id,
                sheet_index: 0,
                stamp: first,
                row: 0,
                col: 0,
                search_text: "first".to_string(),
                display_text: "first".to_string(),
            },
        );
        merge_job(
            &mut state,
            IndexJob::UpdateCell {
                document_id: context.document_id,
                sheet_index: 0,
                stamp: second,
                row: 0,
                col: 0,
                search_text: "second".to_string(),
                display_text: "second".to_string(),
            },
        );
        assert_eq!(state.pending_updates, 1);
        assert_eq!(
            state.pending[&(context.document_id, 0)].incremental[&(0, 0)].search_text,
            "second"
        );

        merge_job(
            &mut state,
            IndexJob::Rebuild {
                document_id: context.document_id,
                sheet_index: 0,
                stamp: second,
            },
        );
        assert!(
            state.pending[&(context.document_id, 0)]
                .incremental
                .is_empty()
        );
        assert!(state.pending[&(context.document_id, 0)].rebuild.is_some());
    }

    #[test]
    fn cancel_removes_pending_jobs_and_owned_indexes_for_one_document() {
        let context = context_for_rows(vec![vec![s("value")]]);
        rebuild_sheet_now(&context);
        context.search.enqueue(IndexJob::Rebuild {
            document_id: context.document_id,
            sheet_index: 0,
            stamp: test_stamp(context.document_id, active_revision(&context), 10),
        });

        context.search.cancel_document_jobs(context.document_id);

        assert!(
            context
                .search
                .scheduler
                .state
                .lock()
                .unwrap()
                .pending
                .is_empty()
        );
        assert!(
            context
                .search
                .scheduler
                .indexes
                .lock()
                .unwrap()
                .document(context.document_id)
                .is_none()
        );
    }

    #[test]
    fn enqueue_drops_jobs_when_workers_are_unavailable() {
        let documents = ActiveDocumentRepository::default();
        let source = Arc::new(RepositorySearchDocumentSource::new(documents));
        let search = isolated_search_adapter(source, false);

        search.enqueue(IndexJob::Rebuild {
            document_id: 1,
            sheet_index: 0,
            stamp: test_stamp(1, 0, 0),
        });

        let state = search.scheduler.state.lock().unwrap();
        assert!(state.pending.is_empty());
        assert_eq!(state.stats.dropped_jobs_no_workers, 1);
    }

    #[test]
    fn stale_rebuild_cannot_install_into_a_replaced_document() {
        let context = context_for_rows(vec![vec![s("old")]]);
        let snapshot = context
            .search
            .scheduler
            .source
            .document_snapshot(context.document_id, None)
            .unwrap()
            .unwrap();
        let stamp = context
            .search
            .prepare_rebuild_all(context.document_id, snapshot.revision, 1)[0]
            .1;
        let search_text =
            snapshot_sheet_search_text(&context.search.scheduler, context.document_id, 0, stamp)
                .unwrap();
        let replacement = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "new.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "New".to_string(),
                    rows: vec![vec![s("new")]],
                    ..Default::default()
                }],
            },
            None,
        );
        context.documents.replace_active_for_test(replacement);

        run_rebuild(
            &context.search.scheduler,
            context.document_id,
            0,
            stamp,
            search_text,
        );

        assert!(
            context
                .search
                .scheduler
                .indexes
                .lock()
                .unwrap()
                .document(context.document_id)
                .is_some_and(|store| store.fresh_sheet_index(0).is_none())
        );
    }
}
