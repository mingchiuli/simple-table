use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex};

use crate::adapters::search_index_store::{SearchIndexRegistry, SearchIndexStamp};
use crate::application::search_ports::SearchDocumentSourcePort;

pub(super) const MAX_PENDING_INDEX_SHEETS: usize = 256;
pub(super) const MAX_PENDING_INDEX_UPDATES_PER_SHEET: usize = 4_096;
pub(super) const MAX_PENDING_INDEX_BYTES_PER_SHEET: usize = 8 * 1024 * 1024;
pub(super) const MAX_PENDING_INDEX_BYTES: usize = 16 * 1024 * 1024;

pub(super) struct IndexScheduler {
    pub(super) state: Mutex<IndexSchedulerState>,
    pub(super) indexes: Arc<Mutex<SearchIndexRegistry>>,
    pub(super) source: Arc<dyn SearchDocumentSourcePort>,
    pub(super) wake: Condvar,
    pub(super) workers_available: AtomicBool,
    pub(super) shutdown: AtomicBool,
}

pub(super) enum IndexJob {
    Rebuild {
        document_id: u64,
        sheet_index: usize,
        stamp: SearchIndexStamp,
    },
    UpdateCell {
        document_id: u64,
        sheet_index: usize,
        stamp: SearchIndexStamp,
        row: usize,
        col: usize,
        search_text: String,
        display_text: String,
    },
}

pub(super) struct CellIndexUpdate {
    pub(super) stamp: SearchIndexStamp,
    pub(super) row: usize,
    pub(super) col: usize,
    pub(super) search_text: String,
    pub(super) display_text: String,
}

pub(super) struct RebuildIndexUpdate {
    pub(super) stamp: SearchIndexStamp,
}

impl IndexJob {
    pub(super) fn document_id(&self) -> u64 {
        match self {
            Self::Rebuild { document_id, .. } | Self::UpdateCell { document_id, .. } => {
                *document_id
            }
        }
    }

    pub(super) fn sheet_index(&self) -> usize {
        match self {
            Self::Rebuild { sheet_index, .. } | Self::UpdateCell { sheet_index, .. } => {
                *sheet_index
            }
        }
    }
}

pub(super) struct SheetPending {
    pub(super) document_id: u64,
    pub(super) rebuild: Option<RebuildIndexUpdate>,
    pub(super) incremental: HashMap<(usize, usize), CellIndexUpdate>,
    pub(super) incremental_bytes: usize,
}

#[derive(Default)]
pub(super) struct IndexSchedulerState {
    pub(super) pending: HashMap<(u64, usize), SheetPending>,
    pub(super) pending_updates: usize,
    pub(super) pending_bytes: usize,
    pub(super) building_jobs: usize,
    pub(super) building_bytes: usize,
    pub(super) stats: SearchSchedulerStats,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SearchSchedulerStats {
    pub(super) queued_jobs: u64,
    pub(super) dropped_jobs_no_workers: u64,
    pub(super) canceled_batches: u64,
    pub(super) drained_batches: u64,
    pub(super) rebuild_jobs: u64,
    pub(super) incremental_jobs: u64,
    pub(super) incremental_fallback_rebuilds: u64,
    pub(super) coalesced_to_rebuilds: u64,
    pub(super) dropped_jobs_at_capacity: u64,
    pub(super) skipped_oversized_rebuilds: u64,
    pub(super) failed_rebuilds: u64,
    pub(super) pending_sheets: usize,
    pub(super) pending_updates: usize,
    pub(super) pending_bytes: usize,
    pub(super) building_jobs: usize,
    pub(super) building_bytes: usize,
    pub(super) peak_building_jobs: usize,
    pub(super) peak_building_bytes: usize,
}

pub(super) fn merge_job(state: &mut IndexSchedulerState, job: IndexJob) {
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
                if latest_seen.is_none_or(|latest| stamp >= latest) || entry.rebuild.is_none() {
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

pub(super) fn update_pending_stats(state: &mut IndexSchedulerState) {
    state.stats.pending_sheets = state.pending.len();
    state.stats.pending_updates = state.pending_updates;
    state.stats.pending_bytes = state.pending_bytes;
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
