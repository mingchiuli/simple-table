use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, OnceLock, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use tantivy::{Term, doc};

use crate::display::DisplayProjection;
use crate::state::search_index::{
    SearchCellText, SearchIndexStamp, build_sheet_index, collect_sheet_search_text,
};
use crate::state::state::ActiveDocumentStore;
#[cfg(test)]
use crate::types::CellValue;
use crate::types::{EditorMutationResponse, EditorPatch, SheetCellChange};

enum IndexJob {
    Rebuild {
        document_id: u64,
        sheet_index: usize,
        stamp: SearchIndexStamp,
        search_text: Vec<SearchCellText>,
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

struct CellIndexUpdate {
    stamp: SearchIndexStamp,
    row: usize,
    col: usize,
    search_text: String,
    display_text: String,
}

struct RebuildIndexUpdate {
    stamp: SearchIndexStamp,
    search_text: Vec<SearchCellText>,
}

impl IndexJob {
    fn document_id(&self) -> u64 {
        match self {
            IndexJob::Rebuild { document_id, .. } | IndexJob::UpdateCell { document_id, .. } => {
                *document_id
            }
        }
    }

    fn sheet_index(&self) -> usize {
        match self {
            IndexJob::Rebuild { sheet_index, .. } | IndexJob::UpdateCell { sheet_index, .. } => {
                *sheet_index
            }
        }
    }

    fn registry(&self) -> &Arc<RwLock<ActiveDocumentStore>> {
        match self {
            IndexJob::Rebuild { registry, .. } | IndexJob::UpdateCell { registry, .. } => registry,
        }
    }
}

struct SheetPending {
    document_id: u64,
    rebuild: Option<RebuildIndexUpdate>,
    incremental: HashMap<(usize, usize), CellIndexUpdate>,
    registry: Arc<RwLock<ActiveDocumentStore>>,
}

const INDEX_DEBOUNCE: Duration = Duration::from_millis(300);

struct IndexScheduler {
    state: Mutex<IndexSchedulerState>,
    wake: Condvar,
}

#[derive(Default)]
struct IndexSchedulerState {
    pending: HashMap<(u64, usize), SheetPending>,
    stats: SearchSchedulerStats,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchSchedulerStats {
    pub queued_jobs: u64,
    pub drained_batches: u64,
    pub rebuild_jobs: u64,
    pub incremental_jobs: u64,
    pub incremental_fallback_rebuilds: u64,
}

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
        let jobs: Vec<(usize, SearchIndexStamp, Vec<SearchCellText>)> = match registry.read() {
            Ok(guard) => guard
                .get(document_id)
                .map(|editor| {
                    let stamp = editor.search_index_stamp();
                    editor
                        .file_data()
                        .sheets
                        .iter()
                        .enumerate()
                        .map(|(sheet_index, sheet)| {
                            (sheet_index, stamp, collect_sheet_search_text(sheet))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        };

        for (sheet_index, stamp, search_text) in jobs {
            self.enqueue(IndexJob::Rebuild {
                document_id,
                sheet_index,
                stamp,
                search_text,
                registry: Arc::clone(registry),
            });
        }
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
        search_text: Vec<SearchCellText>,
        registry: &Arc<RwLock<ActiveDocumentStore>>,
    ) {
        self.enqueue(IndexJob::Rebuild {
            document_id,
            sheet_index,
            stamp,
            search_text,
            registry: Arc::clone(registry),
        });
    }

    pub fn schedule_for_response(
        &self,
        response: &EditorMutationResponse,
        registry: &Arc<RwLock<ActiveDocumentStore>>,
    ) {
        let document_id = response.document_id;
        let Some(stamp) = current_search_stamp(registry, document_id) else {
            return;
        };
        let mut needs_rebuild = false;
        for patch in &response.patches {
            match patch {
                EditorPatch::Cells { changes } => {
                    for change in changes {
                        self.enqueue_cell_update(
                            document_id,
                            change.sheet_index,
                            stamp,
                            change,
                            registry,
                        );
                    }
                }
                EditorPatch::SheetUpdated { patch } => {
                    self.enqueue_rebuild(
                        document_id,
                        patch.sheet_index,
                        stamp,
                        collect_sheet_search_text(&patch.sheet),
                        registry,
                    );
                }
                EditorPatch::SheetShape { .. }
                | EditorPatch::ResyncRequired { .. }
                | EditorPatch::SheetInserted { .. }
                | EditorPatch::SheetDeleted { .. } => needs_rebuild = true,
                EditorPatch::Layout { .. } => {}
            }
        }

        if needs_rebuild {
            self.rebuild_all_sheets_index(registry, document_id);
        }
    }

    fn enqueue(&self, job: IndexJob) {
        if let Ok(mut state) = self.scheduler.state.lock() {
            state.stats.queued_jobs = state.stats.queued_jobs.saturating_add(1);
            merge_job(&mut state.pending, job);
            self.scheduler.wake.notify_one();
        }
    }
}

fn current_search_stamp(
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    document_id: u64,
) -> Option<SearchIndexStamp> {
    registry.read().ok().and_then(|guard| {
        guard
            .get(document_id)
            .map(|editor| editor.search_index_stamp())
    })
}

fn index_scheduler() -> &'static Arc<IndexScheduler> {
    INDEX_SCHEDULER.get_or_init(|| {
        let scheduler = Arc::new(IndexScheduler {
            state: Mutex::new(IndexSchedulerState::default()),
            wake: Condvar::new(),
        });
        let worker_scheduler = Arc::clone(&scheduler);
        thread::Builder::new()
            .name("simple-table-indexer".into())
            .spawn(move || index_worker(&worker_scheduler))
            .expect("failed to spawn index worker thread");
        scheduler
    })
}

fn merge_job(pending: &mut HashMap<(u64, usize), SheetPending>, job: IndexJob) {
    let document_id = job.document_id();
    let sheet_index = job.sheet_index();
    let registry = Arc::clone(job.registry());
    let entry = pending
        .entry((document_id, sheet_index))
        .or_insert_with(|| SheetPending {
            document_id,
            rebuild: None,
            incremental: HashMap::new(),
            registry: Arc::clone(&registry),
        });
    match job {
        IndexJob::Rebuild {
            stamp, search_text, ..
        } => {
            let latest_incremental = entry.incremental.values().map(|update| update.stamp).max();
            let latest_seen = entry
                .rebuild
                .as_ref()
                .map(|rebuild| rebuild.stamp)
                .into_iter()
                .chain(latest_incremental)
                .max();
            if latest_seen.is_none_or(|latest| stamp >= latest) {
                entry.registry = registry;
                entry.rebuild = Some(RebuildIndexUpdate { stamp, search_text });
                entry.incremental.clear();
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
            if entry.rebuild.is_none() {
                let latest_incremental =
                    entry.incremental.values().map(|update| update.stamp).max();
                if latest_incremental.is_some_and(|latest| stamp < latest) {
                    return;
                }
                if latest_incremental.is_some_and(|latest| stamp > latest) {
                    entry.incremental.clear();
                }
                entry.registry = registry;
                entry.incremental.insert(
                    (row, col),
                    CellIndexUpdate {
                        stamp,
                        row,
                        col,
                        search_text,
                        display_text,
                    },
                );
            }
        }
    }
}

fn index_worker(scheduler: &Arc<IndexScheduler>) {
    loop {
        let pending = drain_pending_jobs(scheduler);

        for ((_, sheet_index), pending) in pending {
            if let Some(rebuild) = pending.rebuild {
                record_scheduler_event(scheduler, |stats| {
                    stats.rebuild_jobs = stats.rebuild_jobs.saturating_add(1);
                });
                run_rebuild(
                    pending.document_id,
                    sheet_index,
                    rebuild.stamp,
                    rebuild.search_text,
                    &pending.registry,
                );
                continue;
            }

            if !pending.incremental.is_empty() {
                record_scheduler_event(scheduler, |stats| {
                    stats.incremental_jobs = stats.incremental_jobs.saturating_add(1);
                });
                let latest_stamp = pending
                    .incremental
                    .values()
                    .map(|update| update.stamp)
                    .max()
                    .expect("incremental ops are non-empty");
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
                    if let Some(search_text) = snapshot_sheet_search_text(
                        pending.document_id,
                        sheet_index,
                        latest_stamp,
                        &pending.registry,
                    ) {
                        run_rebuild(
                            pending.document_id,
                            sheet_index,
                            latest_stamp,
                            search_text,
                            &pending.registry,
                        );
                    }
                }
            }
        }
    }
}

fn drain_pending_jobs(scheduler: &Arc<IndexScheduler>) -> HashMap<(u64, usize), SheetPending> {
    let mut state = scheduler
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    while state.pending.is_empty() {
        state = scheduler
            .wake
            .wait(state)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }

    let deadline = Instant::now() + INDEX_DEBOUNCE;
    loop {
        let now = Instant::now();
        if now >= deadline {
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

    let pending = std::mem::take(&mut state.pending);
    state.stats.drained_batches = state.stats.drained_batches.saturating_add(1);
    pending
}

fn record_scheduler_event(
    scheduler: &Arc<IndexScheduler>,
    update: impl FnOnce(&mut SearchSchedulerStats),
) {
    if let Ok(mut state) = scheduler.state.lock() {
        update(&mut state.stats);
    }
}

#[allow(dead_code)]
pub fn search_scheduler_stats() -> SearchSchedulerStats {
    SearchService::global().stats()
}

fn run_rebuild(
    document_id: u64,
    sheet_index: usize,
    stamp: SearchIndexStamp,
    search_text: Vec<SearchCellText>,
    registry: &Arc<RwLock<ActiveDocumentStore>>,
) {
    let built_index = build_sheet_index(&search_text);

    if let Ok(mut guard) = registry.write()
        && let Some(editor_state) = guard.get_mut(document_id)
    {
        editor_state.install_search_index(sheet_index, stamp, built_index);
    }
}

fn snapshot_sheet_search_text(
    document_id: u64,
    sheet_index: usize,
    stamp: SearchIndexStamp,
    registry: &Arc<RwLock<ActiveDocumentStore>>,
) -> Option<Vec<SearchCellText>> {
    match registry.read() {
        Ok(guard) => guard.get(document_id).and_then(|editor| {
            if editor.search_index_stamp() != stamp {
                return None;
            }
            editor
                .file_data()
                .sheets
                .get(sheet_index)
                .map(collect_sheet_search_text)
        }),
        Err(_) => None,
    }
}

fn run_incremental(
    document_id: u64,
    sheet_index: usize,
    registry: &Arc<RwLock<ActiveDocumentStore>>,
    ops: &[CellIndexUpdate],
) -> bool {
    let Some((stamp, handle)) = registry.read().ok().and_then(|guard| {
        let editor = guard.get(document_id)?;
        let stamp = ops.first()?.stamp;
        if ops.iter().any(|op| op.stamp != stamp) {
            return None;
        }
        editor
            .search_writer_handle(sheet_index, stamp)
            .map(|handle| (stamp, handle))
    }) else {
        return false;
    };
    let mut writer = match handle.writer.lock() {
        Ok(writer) => writer,
        Err(_) => return false,
    };

    for op in ops {
        let cell_id = format!("{}:{}", op.row, op.col);
        writer.delete_term(Term::from_field_text(handle.cell_id_field, &cell_id));
        if !op.search_text.is_empty()
            && let Err(error) = writer.add_document(doc!(
                handle.text_field => op.search_text.clone(),
                handle.display_field => op.display_text.clone(),
                handle.row_field => op.row as u64,
                handle.col_field => op.col as u64,
                handle.cell_id_field => cell_id,
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

    if let Ok(mut guard) = registry.write()
        && let Some(editor_state) = guard.get_mut(document_id)
    {
        editor_state.mark_search_sheet_fresh(sheet_index, stamp);
    }

    true
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::EditorCommand;
    use crate::ops::search_ops::do_search;
    use crate::state::editor_state::EditorState;
    use crate::state::state::ActiveDocumentStore;
    use crate::types::{FileData, SearchScope, SheetData};

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
        registry.replace_active(editor);
        (Arc::new(RwLock::new(registry)), document_id)
    }

    fn rows_of(
        registry: &Arc<RwLock<ActiveDocumentStore>>,
        document_id: u64,
        query: &str,
    ) -> Vec<(usize, usize)> {
        let guard = registry.read().unwrap();
        let editor = guard.get(document_id).unwrap();
        let mut rows: Vec<_> = editor
            .indexed_search_sheet(0, query, 10)
            .unwrap_or_default()
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
        let mut rows: Vec<_> = do_search(registry, query, SearchScope::CurrentSheet, Some(0))
            .unwrap()
            .into_iter()
            .map(|result| (result.row, result.col))
            .collect();
        rows.sort();
        rows
    }

    fn current_stamp(
        registry: &Arc<RwLock<ActiveDocumentStore>>,
        document_id: u64,
    ) -> SearchIndexStamp {
        let guard = registry.read().unwrap();
        guard.get(document_id).unwrap().search_index_stamp()
    }

    fn search_text_snapshot(
        registry: &Arc<RwLock<ActiveDocumentStore>>,
        document_id: u64,
    ) -> Vec<SearchCellText> {
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
    fn pending_index_jobs_coalesce_same_cell_update() {
        let (registry, document_id) = make_registry(vec![vec![s("old")]]);
        let stamp = current_stamp(&registry, document_id);
        let mut pending = HashMap::new();

        merge_job(
            &mut pending,
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
            &mut pending,
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

        let sheet = pending.get(&(document_id, 0)).expect("pending sheet");
        assert_eq!(sheet.incremental.len(), 1);
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
        let mut pending = HashMap::new();

        merge_job(
            &mut pending,
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
            &mut pending,
            IndexJob::Rebuild {
                document_id,
                sheet_index: 0,
                stamp,
                search_text: vec![SearchCellText {
                    row: 0,
                    col: 0,
                    text: "latest".to_string(),
                    display: "latest".to_string(),
                }],
                registry,
            },
        );

        let sheet = pending.get(&(document_id, 0)).expect("pending sheet");
        assert_eq!(
            sheet.rebuild.as_ref().map(|rebuild| rebuild.stamp),
            Some(stamp)
        );
        assert!(sheet.incremental.is_empty());
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
            let mut guard = registry.write().unwrap();
            let editor = guard.get_mut(document_id).unwrap();
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
    fn edited_sheet_uses_scan_fallback_until_incremental_index_commits() {
        let (registry, document_id) = make_registry(vec![vec![s("old")]]);
        rebuild_current_sheet(&registry, document_id);
        assert_eq!(rows_of(&registry, document_id, "old"), vec![(0, 0)]);

        let stamp = {
            let mut guard = registry.write().unwrap();
            let editor = guard.get_mut(document_id).unwrap();
            editor
                .execute(EditorCommand::SetCell {
                    sheet_index: 0,
                    row: 0,
                    col: 0,
                    text: "new".to_string(),
                })
                .unwrap();
            editor.search_index_stamp()
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
            let mut guard = registry.write().unwrap();
            let editor = guard.get_mut(document_id).unwrap();
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
            guard.replace_active(new_editor);
        }

        run_rebuild(old_document_id, 0, old_stamp, old_search_text, &registry);

        assert_eq!(rows_of_current_search(&registry, "new"), vec![(0, 0)]);
        assert!(registry.read().unwrap().get(old_document_id).is_none());
    }
}
