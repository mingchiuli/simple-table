use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, RwLock};

use crate::state::search_index::{SearchCellText, SearchIndexStamp};
use crate::state::state::ActiveDocumentStore;

pub(crate) enum IndexJob {
    Rebuild {
        document_id: u64,
        sheet_index: usize,
        stamp: SearchIndexStamp,
        search_text: Arc<[SearchCellText]>,
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
    pub(crate) search_text: Arc<[SearchCellText]>,
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
    pub(crate) registry: Arc<RwLock<ActiveDocumentStore>>,
}

pub(crate) struct IndexScheduler {
    pub(crate) state: Mutex<IndexSchedulerState>,
    pub(crate) wake: Condvar,
}

#[derive(Default)]
pub(crate) struct IndexSchedulerState {
    pub(crate) pending: HashMap<(u64, usize), SheetPending>,
    pub(crate) stats: SearchSchedulerStats,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchSchedulerStats {
    pub queued_jobs: u64,
    pub drained_batches: u64,
    pub rebuild_jobs: u64,
    pub incremental_jobs: u64,
    pub incremental_fallback_rebuilds: u64,
}
