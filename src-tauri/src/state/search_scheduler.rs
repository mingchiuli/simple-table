use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex, RwLock};

use crate::state::search_index::SearchIndexStamp;
use crate::state::state::ActiveDocumentStore;

pub(crate) const MAX_PENDING_INDEX_SHEETS: usize = 256;
pub(crate) const MAX_PENDING_INDEX_UPDATES_PER_SHEET: usize = 4_096;
pub(crate) const MAX_PENDING_INDEX_BYTES_PER_SHEET: usize = 8 * 1024 * 1024;
pub(crate) const MAX_PENDING_INDEX_BYTES: usize = 16 * 1024 * 1024;

pub(crate) enum IndexJob {
    Rebuild {
        document_id: u64,
        sheet_index: usize,
        stamp: SearchIndexStamp,
        registry: Arc<RwLock<ActiveDocumentStore>>,
    },
    UpdateCell {
        document_id: u64,
        sheet_index: usize,
        stamp: SearchIndexStamp,
        row: usize,
        col: usize,
        search_text: String,
        display_text: String,
        registry: Arc<RwLock<ActiveDocumentStore>>,
    },
}

pub(crate) struct CellIndexUpdate {
    pub(crate) stamp: SearchIndexStamp,
    pub(crate) row: usize,
    pub(crate) col: usize,
    pub(crate) search_text: String,
    pub(crate) display_text: String,
}

pub(crate) struct RebuildIndexUpdate {
    pub(crate) stamp: SearchIndexStamp,
}

impl IndexJob {
    pub(crate) fn document_id(&self) -> u64 {
        match self {
            IndexJob::Rebuild { document_id, .. } | IndexJob::UpdateCell { document_id, .. } => {
                *document_id
            }
        }
    }

    pub(crate) fn sheet_index(&self) -> usize {
        match self {
            IndexJob::Rebuild { sheet_index, .. } | IndexJob::UpdateCell { sheet_index, .. } => {
                *sheet_index
            }
        }
    }

    pub(crate) fn registry(&self) -> &Arc<RwLock<ActiveDocumentStore>> {
        match self {
            IndexJob::Rebuild { registry, .. } | IndexJob::UpdateCell { registry, .. } => registry,
        }
    }
}

pub(crate) struct SheetPending {
    pub(crate) document_id: u64,
    pub(crate) rebuild: Option<RebuildIndexUpdate>,
    pub(crate) incremental: HashMap<(usize, usize), CellIndexUpdate>,
    pub(crate) incremental_bytes: usize,
    pub(crate) registry: Arc<RwLock<ActiveDocumentStore>>,
}

pub(crate) struct IndexScheduler {
    pub(crate) state: Mutex<IndexSchedulerState>,
    pub(crate) wake: Condvar,
    pub(crate) workers_available: AtomicBool,
}

#[derive(Default)]
pub(crate) struct IndexSchedulerState {
    pub(crate) pending: HashMap<(u64, usize), SheetPending>,
    pub(crate) pending_updates: usize,
    pub(crate) pending_bytes: usize,
    pub(crate) stats: SearchSchedulerStats,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchSchedulerStats {
    pub queued_jobs: u64,
    pub dropped_jobs_no_workers: u64,
    pub canceled_batches: u64,
    pub drained_batches: u64,
    pub rebuild_jobs: u64,
    pub incremental_jobs: u64,
    pub incremental_fallback_rebuilds: u64,
    pub coalesced_to_rebuilds: u64,
    pub dropped_jobs_at_capacity: u64,
    pub pending_sheets: usize,
    pub pending_updates: usize,
    pub pending_bytes: usize,
}
