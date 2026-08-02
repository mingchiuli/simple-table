use std::collections::HashMap;
#[cfg(test)]
use std::sync::Condvar;
#[cfg(test)]
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::adapters::search_index_backend::SearchIndexReader;
use crate::adapters::search_index_registry::{MAX_RESIDENT_SEARCH_INDEXES, SearchIndexStamp};
#[cfg(test)]
use crate::adapters::search_index_scheduler::IndexSchedulerState;
use crate::adapters::search_index_scheduler::{
    IndexJob, IndexScheduler, merge_job, update_pending_stats,
};
use crate::adapters::search_index_worker::create_index_scheduler;
#[cfg(test)]
use crate::adapters::search_index_worker::{
    MAX_BUILDING_INDEX_BYTES, MAX_INDEXABLE_SHEET_BYTES, index_build_reservation_bytes,
    process_pending_sheet, reserve_index_build, run_rebuild, snapshot_sheet_search_text,
};
use crate::application::search_ports::SearchDocumentSourcePort;
#[cfg(test)]
use crate::domain::{CellValue, SearchCellText};
use crate::domain::{SearchCellIndexUpdate, SearchIndexWork, SearchOutcome, SearchScope};

const SEARCH_SCAN_RESERVATION_BYTES: usize = 24 * 1024 * 1024;

pub(crate) struct SearchIndexRuntime {
    scheduler: Arc<IndexScheduler>,
    scan_work: Arc<Mutex<usize>>,
    workers: Mutex<Vec<thread::JoinHandle<()>>>,
}

impl Drop for SearchIndexRuntime {
    fn drop(&mut self) {
        // Coordinate shutdown with the condition-variable mutex. Without this
        // lock, a worker can observe `shutdown == false`, miss the notification,
        // and then sleep forever while this destructor waits in `join`.
        let scheduler_state = self
            .scheduler
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.scheduler.shutdown.store(true, Ordering::Release);
        self.scheduler
            .workers_available
            .store(false, Ordering::Release);
        self.scheduler.wake.notify_all();
        drop(scheduler_state);
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
    ) -> Option<Arc<dyn SearchIndexReader>> {
        let mut indexes = self.scheduler.indexes.lock().ok()?;
        let index = indexes
            .synchronize_revision(document_id, source_revision)?
            .fresh_sheet_index(sheet_index)?;
        Some(index)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::search_document_source_adapter::RepositorySearchDocumentSource;
    use crate::adapters::search_index_registry::SearchIndexRegistry;
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
    fn failed_index_query_falls_back_to_authoritative_document_scan() {
        let context = context_for_rows(vec![vec![s("authoritative match")]]);
        let revision = active_revision(&context);
        let mut failed_index =
            crate::adapters::search_index_backend::build_sheet_index(&[SearchCellText {
                row: 0,
                col: 0,
                search_text: "authoritative match".to_string(),
                display_text: "authoritative match".to_string(),
            }])
            .expect("index");
        failed_index.fail_queries_for_test("injected index query failure");
        {
            let mut indexes = context.search.scheduler.indexes.lock().unwrap();
            let store = indexes.document_mut(context.document_id);
            store.set_source_revision(revision);
            let stamp = store.sheet_stamp(context.document_id, 0);
            drop(store.install_sheet_index(context.document_id, 0, stamp, Some(failed_index)));
        }

        assert_eq!(search_rows(&context, "authoritative"), vec![(0, 0)]);
        assert!(
            context
                .search
                .scheduler
                .state
                .lock()
                .unwrap()
                .pending
                .contains_key(&(context.document_id, 0))
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
