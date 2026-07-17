use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use tantivy::{Term, doc};

use crate::display::DisplayProjection;
#[cfg(test)]
use crate::state::search_index::SearchQueryPlan;
use crate::state::search_index::{
    MAX_RESIDENT_SEARCH_INDEXES, SearchCellText, SearchIndexStamp, WRITER_ARENA_BYTES,
    build_sheet_index_with_cancel, collect_sheet_search_text, search_position,
};
use crate::state::search_scheduler::{
    CellIndexUpdate, IndexJob, IndexScheduler, IndexSchedulerState, MAX_BUILDING_INDEX_BYTES,
    MAX_INDEXABLE_SHEET_BYTES, MAX_PENDING_INDEX_BYTES, MAX_PENDING_INDEX_BYTES_PER_SHEET,
    MAX_PENDING_INDEX_SHEETS, MAX_PENDING_INDEX_UPDATES_PER_SHEET, RebuildIndexUpdate,
    SearchSchedulerStats, SheetPending,
};
use crate::state::state::{ActiveDocumentStore, DocumentHandle};
#[cfg(test)]
use crate::types::CellValue;
use crate::types::{EditorMutationResponse, EditorPatch, SheetCellChange};

const INDEX_DEBOUNCE: Duration = Duration::from_millis(300);

static INDEX_SCHEDULER: OnceLock<Arc<IndexScheduler>> = OnceLock::new();

#[derive(Clone)]
pub struct SearchService {
    scheduler: Arc<IndexScheduler>,
}

impl SearchService {
    pub fn global() -> Self {
        Self {
            scheduler: Arc::clone(index_scheduler()),
        }
    }

    pub fn stats(&self) -> SearchSchedulerStats {
        self.scheduler
            .state
            .lock()
            .map(|state| state.stats.clone())
            .unwrap_or_default()
    }

    pub fn rebuild_all_sheets_index(
        &self,
        registry: &Arc<RwLock<ActiveDocumentStore>>,
        document_id: u64,
    ) {
        let jobs: Vec<(usize, SearchIndexStamp)> = document_handle(registry, document_id)
            .and_then(|handle| {
                handle.read().ok().map(|editor| {
                    editor
                        .file_data()
                        .sheets
                        .iter()
                        .enumerate()
                        .take(MAX_RESIDENT_SEARCH_INDEXES)
                        .map(|(sheet_index, _sheet)| {
                            let stamp = editor.search_sheet_index_stamp(sheet_index);
                            (sheet_index, stamp)
                        })
                        .collect()
                })
            })
            .unwrap_or_default();

        for (sheet_index, stamp) in jobs {
            self.enqueue(IndexJob::Rebuild {
                document_id,
                sheet_index,
                stamp,
                registry: Arc::clone(registry),
            });
        }
    }

    pub fn rebuild_sheet_index(
        &self,
        registry: &Arc<RwLock<ActiveDocumentStore>>,
        document_id: u64,
        sheet_index: usize,
    ) {
        let Some(stamp) = current_search_stamp(registry, document_id, sheet_index) else {
            return;
        };
        self.enqueue_rebuild(document_id, sheet_index, stamp, registry);
    }

    fn enqueue_cell_update(
        &self,
        document_id: u64,
        sheet_index: usize,
        stamp: SearchIndexStamp,
        change: &SheetCellChange,
        registry: &Arc<RwLock<ActiveDocumentStore>>,
    ) {
        self.enqueue(IndexJob::UpdateCell {
            document_id,
            sheet_index,
            stamp,
            row: change.row,
            col: change.col,
            search_text: DisplayProjection::search_text(
                &change.value,
                change.display_format.as_ref(),
            ),
            display_text: change.display.clone().unwrap_or_else(|| {
                DisplayProjection::display_text(&change.value, change.display_format.as_ref())
            }),
            registry: Arc::clone(registry),
        });
    }

    fn enqueue_rebuild(
        &self,
        document_id: u64,
        sheet_index: usize,
        stamp: SearchIndexStamp,
        registry: &Arc<RwLock<ActiveDocumentStore>>,
    ) {
        self.enqueue(IndexJob::Rebuild {
            document_id,
            sheet_index,
            stamp,
            registry: Arc::clone(registry),
        });
    }

    pub fn schedule_for_response(
        &self,
        response: &EditorMutationResponse,
        registry: &Arc<RwLock<ActiveDocumentStore>>,
    ) {
        let document_id = response.document_id;
        if response.search_index_update.rebuild_all {
            self.rebuild_all_sheets_index(registry, document_id);
            return;
        }

        for sheet_index in &response.search_index_update.rebuild_sheets {
            let Some(stamp) = current_search_stamp(registry, document_id, *sheet_index) else {
                continue;
            };
            self.enqueue_rebuild(document_id, *sheet_index, stamp, registry);
        }

        let mut needs_rebuild = false;
        for patch in &response.patches {
            match patch {
                EditorPatch::Cells { changes } => {
                    for change in changes {
                        let Some(stamp) =
                            current_search_stamp(registry, document_id, change.sheet_index)
                        else {
                            continue;
                        };
                        self.enqueue_cell_update(
                            document_id,
                            change.sheet_index,
                            stamp,
                            change,
                            registry,
                        );
                    }
                }
                EditorPatch::SheetInvalidated { patch } => {
                    let Some(stamp) =
                        current_search_stamp(registry, document_id, patch.sheet_index)
                    else {
                        continue;
                    };
                    self.enqueue_rebuild(document_id, patch.sheet_index, stamp, registry);
                }
                EditorPatch::RowInserted { patch } => {
                    let Some(stamp) =
                        current_search_stamp(registry, document_id, patch.sheet_index)
                    else {
                        continue;
                    };
                    self.enqueue_rebuild(document_id, patch.sheet_index, stamp, registry);
                }
                EditorPatch::RowDeleted { patch } => {
                    let Some(stamp) =
                        current_search_stamp(registry, document_id, patch.sheet_index)
                    else {
                        continue;
                    };
                    self.enqueue_rebuild(document_id, patch.sheet_index, stamp, registry);
                }
                EditorPatch::ColumnInserted { patch } => {
                    let Some(stamp) =
                        current_search_stamp(registry, document_id, patch.sheet_index)
                    else {
                        continue;
                    };
                    self.enqueue_rebuild(document_id, patch.sheet_index, stamp, registry);
                }
                EditorPatch::ColumnDeleted { patch } => {
                    let Some(stamp) =
                        current_search_stamp(registry, document_id, patch.sheet_index)
                    else {
                        continue;
                    };
                    self.enqueue_rebuild(document_id, patch.sheet_index, stamp, registry);
                }
                EditorPatch::ResyncRequired { .. }
                | EditorPatch::SheetInserted { .. }
                | EditorPatch::SheetDeleted { .. }
                | EditorPatch::SheetsReplaced { .. } => needs_rebuild = true,
                EditorPatch::Layout { .. } => {}
            }
        }

        if needs_rebuild {
            self.rebuild_all_sheets_index(registry, document_id);
        }
    }

    pub fn cancel_document_jobs(&self, document_id: u64) {
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

fn current_search_stamp(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
    sheet_index: usize,
) -> Option<SearchIndexStamp> {
    let handle = document_handle(registry, document_id)?;
    handle
        .read()
        .ok()
        .map(|editor| editor.search_sheet_index_stamp(sheet_index))
}

fn index_scheduler() -> &'static Arc<IndexScheduler> {
    INDEX_SCHEDULER.get_or_init(|| {
        let scheduler = Arc::new(IndexScheduler {
            state: Mutex::new(IndexSchedulerState::default()),
            wake: Condvar::new(),
            workers_available: AtomicBool::new(false),
        });
        let worker_count = thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(2)
            .clamp(2, 4);
        for worker_index in 0..worker_count {
            let worker_scheduler = Arc::clone(&scheduler);
            match thread::Builder::new()
                .name(format!("simple-table-indexer-{worker_index}"))
                .spawn(move || index_worker(&worker_scheduler))
            {
                Ok(_) => scheduler.workers_available.store(true, Ordering::Release),
                Err(error) => eprintln!("Failed to spawn search index worker thread: {error}"),
            }
        }
        scheduler
    })
}

fn merge_job(state: &mut IndexSchedulerState, job: IndexJob) {
    let document_id = job.document_id();
    let sheet_index = job.sheet_index();
    let registry = Arc::clone(job.registry());
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
            registry: Arc::clone(&registry),
        });
        let previous_entry_bytes = entry.incremental_bytes;
        let previous_entry_updates = entry.incremental.len();

        match job {
            IndexJob::Rebuild { stamp, .. } => {
                let latest_seen = latest_pending_stamp(entry);
                if latest_seen.is_none_or(|latest| stamp >= latest) {
                    entry.registry = registry;
                    entry.rebuild = Some(RebuildIndexUpdate { stamp });
                    retain_updates_after(entry, stamp);
                } else if entry.rebuild.is_none() {
                    entry.registry = registry;
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
                    entry.registry = registry;
                    entry.rebuild = Some(RebuildIndexUpdate {
                        stamp: latest_stamp,
                    });
                    entry.incremental.clear();
                    entry.incremental_bytes = 0;
                    coalesced_to_rebuild = true;
                } else {
                    entry.registry = registry;
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
    loop {
        let ((_, sheet_index), pending) = drain_pending_job(scheduler);
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
        rebuild_sheet_with_budget(
            scheduler,
            pending.document_id,
            sheet_index,
            latest_stamp,
            &pending.registry,
        );
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
        if !run_incremental(
            pending.document_id,
            sheet_index,
            &pending.registry,
            &updates,
        ) {
            record_scheduler_event(scheduler, |stats| {
                stats.incremental_fallback_rebuilds =
                    stats.incremental_fallback_rebuilds.saturating_add(1);
                stats.rebuild_jobs = stats.rebuild_jobs.saturating_add(1);
            });
            rebuild_sheet_with_budget(
                scheduler,
                pending.document_id,
                sheet_index,
                latest_stamp,
                &pending.registry,
            );
        }
    }
}

fn drain_pending_job(scheduler: &Arc<IndexScheduler>) -> ((u64, usize), SheetPending) {
    let mut state = scheduler
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    loop {
        while state.pending.is_empty() {
            state = scheduler
                .wake
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
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
        return (key, pending);
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
    registry: &Arc<RwLock<ActiveDocumentStore>>,
) {
    let Some(source_bytes) =
        search_sheet_source_estimated_bytes(document_id, sheet_index, stamp, registry)
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
    let Some(search_text) = snapshot_sheet_search_text(document_id, sheet_index, stamp, registry)
    else {
        return;
    };
    run_rebuild(document_id, sheet_index, stamp, search_text, registry);
}

fn search_sheet_source_estimated_bytes(
    document_id: u64,
    sheet_index: usize,
    stamp: SearchIndexStamp,
    registry: &Arc<RwLock<ActiveDocumentStore>>,
) -> Option<usize> {
    let handle = document_handle(registry, document_id)?;
    handle.read().ok().and_then(|editor| {
        (editor.search_sheet_index_stamp(sheet_index) == stamp)
            .then(|| editor.search_sheet_snapshot_estimated_bytes(sheet_index))
            .flatten()
    })
}

#[allow(dead_code)]
pub fn search_scheduler_stats() -> SearchSchedulerStats {
    SearchService::global().stats()
}

fn run_rebuild(
    document_id: u64,
    sheet_index: usize,
    stamp: SearchIndexStamp,
    search_text: Arc<[SearchCellText]>,
    registry: &Arc<RwLock<ActiveDocumentStore>>,
) {
    if !search_stamp_is_current(document_id, sheet_index, stamp, registry) {
        return;
    }

    let built_index = build_sheet_index_with_cancel(&search_text, || {
        search_stamp_is_current(document_id, sheet_index, stamp, registry)
    });

    let retired = {
        let Some(handle) = document_handle(registry, document_id) else {
            drop(built_index);
            return;
        };
        let Ok(mut editor_state) = handle.write() else {
            drop(built_index);
            return;
        };
        editor_state.install_search_index(sheet_index, stamp, built_index)
    };
    drop(retired);
}

fn search_stamp_is_current(
    document_id: u64,
    sheet_index: usize,
    stamp: SearchIndexStamp,
    registry: &Arc<RwLock<ActiveDocumentStore>>,
) -> bool {
    document_handle(registry, document_id)
        .and_then(|handle| {
            handle
                .read()
                .ok()
                .map(|editor| editor.search_sheet_index_stamp(sheet_index) == stamp)
        })
        .unwrap_or(false)
}

fn snapshot_sheet_search_text(
    document_id: u64,
    sheet_index: usize,
    stamp: SearchIndexStamp,
    registry: &Arc<RwLock<ActiveDocumentStore>>,
) -> Option<Arc<[SearchCellText]>> {
    let handle = document_handle(registry, document_id)?;
    let sheet = handle.read().ok().and_then(|editor| {
        if editor.search_sheet_index_stamp(sheet_index) != stamp {
            return None;
        }
        editor.search_sheet_data(sheet_index)
    })?;
    Some(Arc::from(collect_sheet_search_text(&sheet)))
}

fn run_incremental(
    document_id: u64,
    sheet_index: usize,
    registry: &Arc<RwLock<ActiveDocumentStore>>,
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
    let Some(writer_handle) = document_handle(registry, document_id).and_then(|handle| {
        let editor = handle.read().ok()?;
        editor.search_writer_handle(sheet_index, latest_stamp)
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

    let retired = document_handle(registry, document_id).and_then(|handle| {
        handle
            .write()
            .ok()
            .map(|mut editor_state| editor_state.mark_search_sheet_fresh(sheet_index, latest_stamp))
    });
    drop(retired);

    true
}

fn document_handle(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
) -> Option<Arc<DocumentHandle>> {
    registry.read().ok()?.handle(document_id)
}

pub fn spawn_rebuild_all_sheets_index(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
) {
    SearchService::global().rebuild_all_sheets_index(registry, document_id);
}

pub fn schedule_index_for_response(
    response: &EditorMutationResponse,
    registry: &Arc<RwLock<ActiveDocumentStore>>,
) {
    SearchService::global().schedule_for_response(response, registry);
}

pub fn cancel_index_jobs_for_document(document_id: u64) {
    SearchService::global().cancel_document_jobs(document_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use crate::ops::EditorCommand;
    use crate::ops::search_ops::do_search;
    use crate::state::editor_state::EditorState;
    use crate::state::state::ActiveDocumentStore;
    use crate::types::{
        CellFormatProjection, FileData, ReadOnlyRichProjection, SearchScope, SheetData,
    };
    use serde_json::Value;

    fn s(value: &str) -> CellValue {
        CellValue::String(value.to_string())
    }

    fn make_registry(rows: Vec<Vec<CellValue>>) -> (Arc<RwLock<ActiveDocumentStore>>, u64) {
        let editor = EditorState::with_workbook(
            FileData {
                path: String::new(),
                file_name: "test.xlsx".to_string(),
                sheets: vec![SheetData {
                    name: "Test".to_string(),
                    rows,
                    ..Default::default()
                }],
            },
            None,
        );
        let document_id = editor.document_id();
        let mut registry = ActiveDocumentStore::new_for_test();
        registry.replace_active_for_test(editor);
        (Arc::new(RwLock::new(registry)), document_id)
    }

    fn isolated_search_service() -> SearchService {
        SearchService {
            scheduler: Arc::new(IndexScheduler {
                state: Mutex::new(IndexSchedulerState::default()),
                wake: Condvar::new(),
                workers_available: AtomicBool::new(true),
            }),
        }
    }

    fn isolated_search_service_without_workers() -> SearchService {
        SearchService {
            scheduler: Arc::new(IndexScheduler {
                state: Mutex::new(IndexSchedulerState::default()),
                wake: Condvar::new(),
                workers_available: AtomicBool::new(false),
            }),
        }
    }

    #[test]
    fn index_build_reservations_are_bounded_and_released() {
        let service = isolated_search_service();
        let reservation = reserve_index_build(&service.scheduler, MAX_BUILDING_INDEX_BYTES)
            .expect("build reservation");
        {
            let state = service.scheduler.state.lock().expect("scheduler state");
            assert_eq!(state.building_jobs, 1);
            assert_eq!(state.building_bytes, MAX_BUILDING_INDEX_BYTES);
            assert_eq!(state.stats.peak_building_jobs, 1);
            assert_eq!(state.stats.peak_building_bytes, MAX_BUILDING_INDEX_BYTES);
        }

        drop(reservation);

        let state = service.scheduler.state.lock().expect("scheduler state");
        assert_eq!(state.building_jobs, 0);
        assert_eq!(state.building_bytes, 0);
    }

    #[test]
    fn oversized_sheet_sources_do_not_enter_the_index_builder() {
        assert!(index_build_reservation_bytes(MAX_INDEXABLE_SHEET_BYTES).is_some());
        assert!(index_build_reservation_bytes(MAX_INDEXABLE_SHEET_BYTES + 1).is_none());
    }

    #[test]
    fn on_demand_rebuild_enqueues_only_the_requested_sheet() {
        let (registry, document_id) = make_registry(vec![vec![s("needle")]]);
        let service = isolated_search_service();

        service.rebuild_sheet_index(&registry, document_id, 0);

        let state = service.scheduler.state.lock().expect("scheduler state");
        assert_eq!(state.stats.queued_jobs, 1);
        assert!(state.pending.contains_key(&(document_id, 0)));
    }

    fn rows_of(
        registry: &Arc<RwLock<ActiveDocumentStore>>,
        document_id: u64,
        query: &str,
    ) -> Vec<(usize, usize)> {
        let guard = registry.read().unwrap();
        let editor = guard.get(document_id).unwrap();
        let plan = SearchQueryPlan::new(query).expect("query plan");
        let cells = if let Some(index) = editor.indexed_search_sheet(0) {
            index.search(&plan, 10)
        } else {
            collect_sheet_search_text(&editor.search_sheet_data(0).expect("search sheet"))
                .into_iter()
                .filter(|cell| plan.matches(&cell.search_text))
                .take(10)
                .collect()
        };
        let mut rows: Vec<_> = cells
            .into_iter()
            .map(|position| (position.row, position.col))
            .collect();
        rows.sort();
        rows
    }

    fn rows_of_current_search(
        registry: &Arc<RwLock<ActiveDocumentStore>>,
        query: &str,
    ) -> Vec<(usize, usize)> {
        let (document_id, revision) = active_search_context(registry);
        let mut rows: Vec<_> = do_search(
            registry,
            document_id,
            revision,
            query,
            SearchScope::CurrentSheet,
            Some(0),
        )
        .unwrap()
        .results
        .into_iter()
        .map(|result| (result.row, result.col))
        .collect();
        rows.sort();
        rows
    }

    fn values_of_current_search(
        registry: &Arc<RwLock<ActiveDocumentStore>>,
        query: &str,
    ) -> Vec<String> {
        let (document_id, revision) = active_search_context(registry);
        do_search(
            registry,
            document_id,
            revision,
            query,
            SearchScope::CurrentSheet,
            Some(0),
        )
        .unwrap()
        .results
        .into_iter()
        .map(|result| result.value)
        .collect()
    }

    fn active_search_context(registry: &Arc<RwLock<ActiveDocumentStore>>) -> (u64, u64) {
        let guard = registry.read().unwrap();
        let editor = guard.active().unwrap();
        (editor.document_id(), editor.revision())
    }

    fn current_stamp(
        registry: &Arc<RwLock<ActiveDocumentStore>>,
        document_id: u64,
    ) -> SearchIndexStamp {
        let guard = registry.read().unwrap();
        guard.get(document_id).unwrap().search_sheet_index_stamp(0)
    }

    fn search_text_snapshot(
        registry: &Arc<RwLock<ActiveDocumentStore>>,
        document_id: u64,
    ) -> Arc<[SearchCellText]> {
        snapshot_sheet_search_text(
            document_id,
            0,
            current_stamp(registry, document_id),
            registry,
        )
        .expect("search text snapshot")
    }

    fn rebuild_current_sheet(registry: &Arc<RwLock<ActiveDocumentStore>>, document_id: u64) {
        run_rebuild(
            document_id,
            0,
            current_stamp(registry, document_id),
            search_text_snapshot(registry, document_id),
            registry,
        );
    }

    #[test]
    fn search_rejects_stale_document_revision() {
        let (registry, document_id) = make_registry(vec![vec![s("old")]]);
        let revision = {
            let guard = registry.read().unwrap();
            guard.get(document_id).unwrap().revision()
        };
        {
            let handle = document_handle(&registry, document_id).unwrap();
            handle
                .write()
                .unwrap()
                .execute(EditorCommand::SetCell {
                    sheet_index: 0,
                    row: 0,
                    col: 0,
                    text: "new".to_string(),
                })
                .unwrap();
        }

        let error = do_search(
            &registry,
            document_id,
            revision,
            "new",
            SearchScope::CurrentSheet,
            Some(0),
        )
        .expect_err("stale search context should be rejected");

        assert!(matches!(error, AppError::DocumentStateInvalid(_)));
    }

    #[test]
    fn search_sources_remain_usable_without_holding_the_document_lock() {
        let (registry, document_id) = make_registry(vec![vec![s("alpha beta")]]);
        let snapshot = {
            let guard = registry.read().unwrap();
            guard
                .get(document_id)
                .unwrap()
                .search_sheet_data(0)
                .expect("search sheet")
        };
        let write_guard = registry
            .try_write()
            .expect("snapshot does not retain registry lock");
        let snapshot_cells = collect_sheet_search_text(&snapshot);
        assert_eq!(snapshot_cells[0].search_text, "alpha beta");
        drop(write_guard);

        rebuild_current_sheet(&registry, document_id);
        let indexed = {
            let guard = registry.read().unwrap();
            guard
                .get(document_id)
                .unwrap()
                .indexed_search_sheet(0)
                .expect("indexed source")
        };
        let _write_guard = registry
            .try_write()
            .expect("index does not retain registry lock");
        let plan = SearchQueryPlan::new("alpha beta").expect("query plan");
        let cells = indexed.search(&plan, 10);
        assert_eq!(cells.len(), 1);
    }

    #[test]
    fn pending_index_jobs_coalesce_same_cell_update() {
        let (registry, document_id) = make_registry(vec![vec![s("old")]]);
        let stamp = current_stamp(&registry, document_id);
        let mut state = IndexSchedulerState::default();

        merge_job(
            &mut state,
            IndexJob::UpdateCell {
                document_id,
                sheet_index: 0,
                stamp,
                row: 0,
                col: 0,
                search_text: "intermediate".to_string(),
                display_text: "intermediate".to_string(),
                registry: Arc::clone(&registry),
            },
        );
        merge_job(
            &mut state,
            IndexJob::UpdateCell {
                document_id,
                sheet_index: 0,
                stamp,
                row: 0,
                col: 0,
                search_text: "latest".to_string(),
                display_text: "latest".to_string(),
                registry,
            },
        );

        let sheet = state.pending.get(&(document_id, 0)).expect("pending sheet");
        assert_eq!(sheet.incremental.len(), 1);
        assert_eq!(state.pending_updates, 1);
        assert_eq!(state.pending_bytes, sheet.incremental_bytes);
        assert_eq!(
            sheet
                .incremental
                .get(&(0, 0))
                .map(|update| update.search_text.as_str()),
            Some("latest")
        );
    }

    #[test]
    fn pending_rebuild_supersedes_cell_updates() {
        let (registry, document_id) = make_registry(vec![vec![s("old")]]);
        let stamp = current_stamp(&registry, document_id);
        let mut state = IndexSchedulerState::default();

        merge_job(
            &mut state,
            IndexJob::UpdateCell {
                document_id,
                sheet_index: 0,
                stamp,
                row: 0,
                col: 0,
                search_text: "latest".to_string(),
                display_text: "latest".to_string(),
                registry: Arc::clone(&registry),
            },
        );
        merge_job(
            &mut state,
            IndexJob::Rebuild {
                document_id,
                sheet_index: 0,
                stamp,
                registry,
            },
        );

        let sheet = state.pending.get(&(document_id, 0)).expect("pending sheet");
        assert_eq!(
            sheet.rebuild.as_ref().map(|rebuild| rebuild.stamp),
            Some(stamp)
        );
        assert!(sheet.incremental.is_empty());
        assert_eq!(state.pending_updates, 0);
        assert_eq!(state.pending_bytes, 0);
    }

    #[test]
    fn pending_updates_collapse_to_rebuild_at_the_sheet_entry_limit() {
        let (registry, document_id) = make_registry(vec![vec![s("old")]]);
        let stamp = current_stamp(&registry, document_id);
        let mut state = IndexSchedulerState::default();

        for row in 0..=MAX_PENDING_INDEX_UPDATES_PER_SHEET {
            merge_job(
                &mut state,
                IndexJob::UpdateCell {
                    document_id,
                    sheet_index: 0,
                    stamp,
                    row,
                    col: 0,
                    search_text: "value".to_string(),
                    display_text: "value".to_string(),
                    registry: Arc::clone(&registry),
                },
            );
        }

        let sheet = state.pending.get(&(document_id, 0)).expect("pending sheet");
        assert_eq!(
            sheet.rebuild.as_ref().map(|rebuild| rebuild.stamp),
            Some(stamp)
        );
        assert!(sheet.incremental.is_empty());
        assert_eq!(state.pending_updates, 0);
        assert_eq!(state.pending_bytes, 0);
        assert_eq!(state.stats.coalesced_to_rebuilds, 1);
    }

    #[test]
    fn a_large_pending_update_collapses_to_rebuild_at_the_byte_limit() {
        let (registry, document_id) = make_registry(vec![vec![s("old")]]);
        let stamp = current_stamp(&registry, document_id);
        let mut state = IndexSchedulerState::default();
        let oversized = "x".repeat(MAX_PENDING_INDEX_BYTES_PER_SHEET);

        merge_job(
            &mut state,
            IndexJob::UpdateCell {
                document_id,
                sheet_index: 0,
                stamp,
                row: 0,
                col: 0,
                search_text: oversized,
                display_text: String::new(),
                registry,
            },
        );

        let sheet = state.pending.get(&(document_id, 0)).expect("pending sheet");
        assert!(sheet.rebuild.is_some());
        assert!(sheet.incremental.is_empty());
        assert_eq!(state.stats.pending_updates, 0);
        assert_eq!(state.stats.pending_bytes, 0);
        assert_eq!(state.stats.coalesced_to_rebuilds, 1);
    }

    #[test]
    fn cancel_document_jobs_removes_only_matching_pending_batches() {
        let service = isolated_search_service();
        let (old_registry, old_document_id) = make_registry(vec![vec![s("old")]]);
        let (new_registry, new_document_id) = make_registry(vec![vec![s("new")]]);
        let old_stamp = current_stamp(&old_registry, old_document_id);
        let new_stamp = current_stamp(&new_registry, new_document_id);

        service.enqueue(IndexJob::Rebuild {
            document_id: old_document_id,
            sheet_index: 0,
            stamp: old_stamp,
            registry: Arc::clone(&old_registry),
        });
        service.enqueue(IndexJob::UpdateCell {
            document_id: new_document_id,
            sheet_index: 0,
            stamp: new_stamp,
            row: 0,
            col: 0,
            search_text: "newer".to_string(),
            display_text: "newer".to_string(),
            registry: Arc::clone(&new_registry),
        });

        service.cancel_document_jobs(old_document_id);

        let state = service.scheduler.state.lock().unwrap();
        assert!(
            !state
                .pending
                .keys()
                .any(|(document_id, _)| *document_id == old_document_id)
        );
        assert!(
            state
                .pending
                .keys()
                .any(|(document_id, _)| *document_id == new_document_id)
        );
        assert_eq!(state.stats.canceled_batches, 1);
    }

    #[test]
    fn enqueue_drops_index_jobs_when_no_workers_are_available() {
        let service = isolated_search_service_without_workers();
        let (registry, document_id) = make_registry(vec![vec![s("old")]]);
        let stamp = current_stamp(&registry, document_id);

        service.enqueue(IndexJob::Rebuild {
            document_id,
            sheet_index: 0,
            stamp,
            registry,
        });

        let state = service.scheduler.state.lock().unwrap();
        assert!(state.pending.is_empty());
        assert_eq!(state.stats.queued_jobs, 0);
        assert_eq!(state.stats.dropped_jobs_no_workers, 1);
    }

    #[test]
    fn rebuild_searches_existing_content() {
        let (registry, document_id) = make_registry(vec![
            vec![s("apple"), s("banana")],
            vec![s("cherry"), s("durian")],
        ]);
        rebuild_current_sheet(&registry, document_id);

        assert_eq!(rows_of(&registry, document_id, "apple"), vec![(0, 0)]);
        assert_eq!(rows_of(&registry, document_id, "durian"), vec![(1, 1)]);
    }

    #[test]
    fn incremental_update_replaces_old_value() {
        let (registry, document_id) = make_registry(vec![vec![s("apple"), s("banana")]]);
        rebuild_current_sheet(&registry, document_id);
        {
            let handle = document_handle(&registry, document_id).unwrap();
            let mut editor = handle.write().unwrap();
            editor
                .execute(EditorCommand::SetCell {
                    sheet_index: 0,
                    row: 0,
                    col: 0,
                    text: "orange".to_string(),
                })
                .unwrap();
        }

        let ok = run_incremental(
            document_id,
            0,
            &registry,
            &[CellIndexUpdate {
                stamp: current_stamp(&registry, document_id),
                row: 0,
                col: 0,
                search_text: "orange".to_string(),
                display_text: "orange".to_string(),
            }],
        );

        assert!(ok);
        assert!(rows_of(&registry, document_id, "apple").is_empty());
        assert_eq!(rows_of(&registry, document_id, "orange"), vec![(0, 0)]);
        assert_eq!(rows_of(&registry, document_id, "banana"), vec![(0, 1)]);
    }

    #[test]
    fn incremental_updates_can_span_sheet_revisions() {
        let (registry, document_id) = make_registry(vec![vec![s("apple"), s("banana")]]);
        rebuild_current_sheet(&registry, document_id);

        let first_stamp = {
            let handle = document_handle(&registry, document_id).unwrap();
            let mut editor = handle.write().unwrap();
            editor
                .execute(EditorCommand::SetCell {
                    sheet_index: 0,
                    row: 0,
                    col: 0,
                    text: "orange".to_string(),
                })
                .unwrap();
            editor.search_sheet_index_stamp(0)
        };
        let second_stamp = {
            let handle = document_handle(&registry, document_id).unwrap();
            let mut editor = handle.write().unwrap();
            editor
                .execute(EditorCommand::SetCell {
                    sheet_index: 0,
                    row: 0,
                    col: 1,
                    text: "pear".to_string(),
                })
                .unwrap();
            editor.search_sheet_index_stamp(0)
        };

        let ok = run_incremental(
            document_id,
            0,
            &registry,
            &[
                CellIndexUpdate {
                    stamp: first_stamp,
                    row: 0,
                    col: 0,
                    search_text: "orange".to_string(),
                    display_text: "orange".to_string(),
                },
                CellIndexUpdate {
                    stamp: second_stamp,
                    row: 0,
                    col: 1,
                    search_text: "pear".to_string(),
                    display_text: "pear".to_string(),
                },
            ],
        );

        assert!(ok);
        assert!(rows_of(&registry, document_id, "apple").is_empty());
        assert!(rows_of(&registry, document_id, "banana").is_empty());
        assert_eq!(rows_of(&registry, document_id, "orange"), vec![(0, 0)]);
        assert_eq!(rows_of(&registry, document_id, "pear"), vec![(0, 1)]);
    }

    #[test]
    fn edited_sheet_uses_scan_fallback_until_incremental_index_commits() {
        let (registry, document_id) = make_registry(vec![vec![s("old")]]);
        rebuild_current_sheet(&registry, document_id);
        assert_eq!(rows_of(&registry, document_id, "old"), vec![(0, 0)]);

        let stamp = {
            let handle = document_handle(&registry, document_id).unwrap();
            let mut editor = handle.write().unwrap();
            editor
                .execute(EditorCommand::SetCell {
                    sheet_index: 0,
                    row: 0,
                    col: 0,
                    text: "new".to_string(),
                })
                .unwrap();
            editor.search_sheet_index_stamp(0)
        };

        assert!(rows_of_current_search(&registry, "old").is_empty());
        assert_eq!(rows_of_current_search(&registry, "new"), vec![(0, 0)]);

        let ok = run_incremental(
            document_id,
            0,
            &registry,
            &[CellIndexUpdate {
                stamp,
                row: 0,
                col: 0,
                search_text: "new".to_string(),
                display_text: "new".to_string(),
            }],
        );

        assert!(ok);
        assert!(rows_of(&registry, document_id, "old").is_empty());
        assert_eq!(rows_of(&registry, document_id, "new"), vec![(0, 0)]);
    }

    #[test]
    fn stale_index_search_falls_back_to_current_rows() {
        let (registry, document_id) = make_registry(vec![vec![s("apple")]]);
        rebuild_current_sheet(&registry, document_id);
        assert_eq!(rows_of(&registry, document_id, "apple"), vec![(0, 0)]);

        {
            let handle = document_handle(&registry, document_id).unwrap();
            let mut editor = handle.write().unwrap();
            editor
                .execute(EditorCommand::SetCell {
                    sheet_index: 0,
                    row: 0,
                    col: 0,
                    text: "orange".to_string(),
                })
                .unwrap();
            editor.mark_search_index_stale();
        }

        assert!(rows_of_current_search(&registry, "apple").is_empty());
        assert_eq!(rows_of_current_search(&registry, "orange"), vec![(0, 0)]);

        rebuild_current_sheet(&registry, document_id);
        assert!(rows_of(&registry, document_id, "apple").is_empty());
        assert_eq!(rows_of(&registry, document_id, "orange"), vec![(0, 0)]);
    }

    #[test]
    fn scan_fallback_matches_raw_search_text_but_returns_display_text() {
        let sheet = SheetData {
            name: "Test".to_string(),
            rows: vec![vec![CellValue::Number(Value::from(0.4))]],
            rich: ReadOnlyRichProjection {
                cell_formats: HashMap::from([(
                    "A1".to_string(),
                    CellFormatProjection {
                        number_format: Some("0%".to_string()),
                        style_id: None,
                    },
                )]),
                ..Default::default()
            },
            ..Default::default()
        };
        let editor = EditorState::with_workbook(
            FileData {
                path: String::new(),
                file_name: "formatted.xlsx".to_string(),
                sheets: vec![sheet],
            },
            None,
        );
        let document_id = editor.document_id();
        let mut store = ActiveDocumentStore::new_for_test();
        store.replace_active_for_test(editor);
        let registry = Arc::new(RwLock::new(store));

        assert_eq!(rows_of_current_search(&registry, "0.4"), vec![(0, 0)]);
        assert_eq!(values_of_current_search(&registry, "0.4"), vec!["40%"]);

        rebuild_current_sheet(&registry, document_id);
        assert_eq!(rows_of_current_search(&registry, "0.4"), vec![(0, 0)]);
        assert_eq!(values_of_current_search(&registry, "0.4"), vec!["40%"]);
    }

    #[test]
    fn stale_rebuild_job_does_not_write_into_replaced_active_document() {
        let (registry, old_document_id) = make_registry(vec![vec![s("old")]]);
        let old_stamp = current_stamp(&registry, old_document_id);
        let old_search_text = search_text_snapshot(&registry, old_document_id);

        let new_editor = EditorState::with_workbook(
            FileData {
                path: String::new(),
                file_name: "new.xlsx".to_string(),
                sheets: vec![SheetData {
                    name: "New".to_string(),
                    rows: vec![vec![s("new")]],
                    ..Default::default()
                }],
            },
            None,
        );
        {
            let mut guard = registry.write().unwrap();
            guard.replace_active_for_test(new_editor);
        }

        run_rebuild(old_document_id, 0, old_stamp, old_search_text, &registry);

        assert_eq!(rows_of_current_search(&registry, "new"), vec![(0, 0)]);
        assert!(registry.read().unwrap().get(old_document_id).is_none());
    }
}
