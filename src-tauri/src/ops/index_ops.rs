use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use tantivy::{Term, doc};

use crate::state::search_index::{
    SearchCellText, SearchIndexStamp, build_sheet_index, collect_sheet_search_text,
};
use crate::state::state::DocumentRegistry;
use crate::types::{CellValue, EditorMutationResponse, EditorPatch};

enum IndexJob {
    Rebuild {
        document_id: u64,
        sheet_index: usize,
        stamp: SearchIndexStamp,
        registry: Arc<RwLock<DocumentRegistry>>,
    },
    UpdateCell {
        document_id: u64,
        sheet_index: usize,
        stamp: SearchIndexStamp,
        row: usize,
        col: usize,
        new_text: String,
        registry: Arc<RwLock<DocumentRegistry>>,
    },
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

    fn stamp(&self) -> SearchIndexStamp {
        match self {
            IndexJob::Rebuild { stamp, .. } | IndexJob::UpdateCell { stamp, .. } => *stamp,
        }
    }

    fn registry(&self) -> &Arc<RwLock<DocumentRegistry>> {
        match self {
            IndexJob::Rebuild { registry, .. } | IndexJob::UpdateCell { registry, .. } => registry,
        }
    }
}

struct SheetPending {
    document_id: u64,
    rebuild: Option<SearchIndexStamp>,
    incremental: Vec<IndexJob>,
    registry: Arc<RwLock<DocumentRegistry>>,
}

const INDEX_DEBOUNCE: Duration = Duration::from_millis(300);

static INDEX_QUEUE: OnceLock<mpsc::Sender<IndexJob>> = OnceLock::new();

fn index_queue() -> &'static mpsc::Sender<IndexJob> {
    INDEX_QUEUE.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<IndexJob>();
        thread::Builder::new()
            .name("simple-table-indexer".into())
            .spawn(move || index_worker(rx))
            .expect("failed to spawn index worker thread");
        tx
    })
}

fn merge_job(pending: &mut HashMap<(u64, usize), SheetPending>, job: IndexJob) {
    let document_id = job.document_id();
    let sheet_index = job.sheet_index();
    let registry = job.registry().clone();
    let entry = pending
        .entry((document_id, sheet_index))
        .or_insert_with(|| SheetPending {
            document_id,
            rebuild: None,
            incremental: Vec::new(),
            registry: registry.clone(),
        });
    match job {
        IndexJob::Rebuild { stamp, .. } => {
            let latest_incremental = entry.incremental.iter().map(IndexJob::stamp).max();
            let latest_seen = entry.rebuild.into_iter().chain(latest_incremental).max();
            if latest_seen.is_none_or(|latest| stamp >= latest) {
                entry.registry = registry;
                entry.rebuild = Some(stamp);
                entry.incremental.clear();
            }
        }
        other => {
            if entry.rebuild.is_none() {
                entry.registry = registry;
                entry.incremental.push(other);
            }
        }
    }
}

fn index_worker(rx: mpsc::Receiver<IndexJob>) {
    loop {
        let first = match rx.recv() {
            Ok(job) => job,
            Err(_) => return,
        };

        let mut pending = HashMap::new();
        merge_job(&mut pending, first);

        let deadline = Instant::now() + INDEX_DEBOUNCE;
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            match rx.recv_timeout(deadline - now) {
                Ok(job) => merge_job(&mut pending, job),
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }

        for ((_, sheet_index), pending) in pending {
            if let Some(stamp) = pending.rebuild {
                run_rebuild(pending.document_id, sheet_index, stamp, &pending.registry);
            } else if !pending.incremental.is_empty()
                && !run_incremental(
                    pending.document_id,
                    sheet_index,
                    &pending.registry,
                    &pending.incremental,
                )
            {
                let latest_stamp = pending
                    .incremental
                    .iter()
                    .map(IndexJob::stamp)
                    .max()
                    .expect("incremental ops are non-empty");
                run_rebuild(
                    pending.document_id,
                    sheet_index,
                    latest_stamp,
                    &pending.registry,
                );
            }
        }
    }
}

fn run_rebuild(
    document_id: u64,
    sheet_index: usize,
    stamp: SearchIndexStamp,
    registry: &Arc<RwLock<DocumentRegistry>>,
) {
    let search_text_snapshot: Option<Vec<SearchCellText>> = match registry.read() {
        Ok(guard) => guard.get(document_id).and_then(|editor| {
            if editor.search_index_stamp() != stamp {
                return None;
            }
            editor
                .file_data()
                .sheets
                .get(sheet_index)
                .map(|sheet| collect_sheet_search_text(&sheet.rows))
        }),
        Err(_) => None,
    };
    let Some(search_text) = search_text_snapshot else {
        return;
    };
    let built_index = build_sheet_index(&search_text);

    if let Ok(mut guard) = registry.write()
        && let Some(editor_state) = guard.get_mut(document_id)
    {
        editor_state.install_search_index(sheet_index, stamp, built_index);
    }
}

fn run_incremental(
    document_id: u64,
    sheet_index: usize,
    registry: &Arc<RwLock<DocumentRegistry>>,
    ops: &[IndexJob],
) -> bool {
    let Some((stamp, handle)) = registry.read().ok().and_then(|guard| {
        let editor = guard.get(document_id)?;
        let stamp = ops.first()?.stamp();
        if ops.iter().any(|op| op.stamp() != stamp) {
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
        match op {
            IndexJob::UpdateCell {
                row, col, new_text, ..
            } => {
                let cell_id = format!("{}:{}", row, col);
                writer.delete_term(Term::from_field_text(handle.cell_id_field, &cell_id));
                if !new_text.is_empty()
                    && let Err(error) = writer.add_document(doc!(
                        handle.text_field => new_text.clone(),
                        handle.row_field => *row as u64,
                        handle.col_field => *col as u64,
                        handle.cell_id_field => cell_id,
                    ))
                {
                    eprintln!("incremental add_document failed: {error:?}");
                    return false;
                }
            }
            IndexJob::Rebuild { .. } => unreachable!("rebuild handled by dispatcher"),
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

pub fn spawn_rebuild_all_sheets_index(registry: Arc<RwLock<DocumentRegistry>>, document_id: u64) {
    let (count, stamp) = match registry.read() {
        Ok(guard) => guard
            .get(document_id)
            .map(|editor| (editor.file_data().sheets.len(), editor.search_index_stamp()))
            .unwrap_or((0, SearchIndexStamp::default())),
        Err(_) => (0, SearchIndexStamp::default()),
    };

    let queue = index_queue();
    for sheet_index in 0..count {
        let _ = queue.send(IndexJob::Rebuild {
            document_id,
            sheet_index,
            stamp,
            registry: registry.clone(),
        });
    }
}

pub fn spawn_update_cell_index(
    document_id: u64,
    sheet_index: usize,
    row: usize,
    col: usize,
    new_value: &CellValue,
    registry: Arc<RwLock<DocumentRegistry>>,
) {
    let stamp = match registry.read() {
        Ok(guard) => guard
            .get(document_id)
            .map(|editor| editor.search_index_stamp())
            .unwrap_or_default(),
        Err(_) => SearchIndexStamp::default(),
    };
    let _ = index_queue().send(IndexJob::UpdateCell {
        document_id,
        sheet_index,
        stamp,
        row,
        col,
        new_text: new_value.to_display_string(),
        registry,
    });
}

pub fn schedule_index_for_response(
    response: &EditorMutationResponse,
    registry: Arc<RwLock<DocumentRegistry>>,
) {
    let document_id = response.document_id;
    let mut needs_rebuild = false;
    for patch in &response.patches {
        match patch {
            EditorPatch::Cells { changes } => {
                for change in changes {
                    spawn_update_cell_index(
                        document_id,
                        change.sheet_index,
                        change.row,
                        change.col,
                        &change.value,
                        registry.clone(),
                    );
                }
            }
            EditorPatch::FullSnapshot { .. } | EditorPatch::SheetSnapshot { .. } => {
                needs_rebuild = true
            }
            EditorPatch::Layout { .. } => {}
        }
    }

    if needs_rebuild {
        spawn_rebuild_all_sheets_index(registry, document_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::EditorCommand;
    use crate::state::editor_state::{EditorState, SearchSource};
    use crate::state::state::DocumentRegistry;
    use crate::types::{FileData, SheetData};

    fn s(value: &str) -> CellValue {
        CellValue::String(value.to_string())
    }

    fn make_registry(rows: Vec<Vec<CellValue>>) -> (Arc<RwLock<DocumentRegistry>>, u64) {
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
        let mut registry = DocumentRegistry::new_for_test();
        registry.replace_active(editor);
        (Arc::new(RwLock::new(registry)), document_id)
    }

    fn rows_of(
        registry: &Arc<RwLock<DocumentRegistry>>,
        document_id: u64,
        query: &str,
    ) -> Vec<(usize, usize)> {
        let guard = registry.read().unwrap();
        let editor = guard.get(document_id).unwrap();
        let mut rows: Vec<_> = editor
            .search_sheet(0, query, 10)
            .positions
            .iter()
            .map(|position| (position.row, position.col))
            .collect();
        rows.sort();
        rows
    }

    fn search_source(
        registry: &Arc<RwLock<DocumentRegistry>>,
        document_id: u64,
        query: &str,
    ) -> SearchSource {
        let guard = registry.read().unwrap();
        guard
            .get(document_id)
            .unwrap()
            .search_sheet(0, query, 10)
            .source
    }

    fn current_stamp(
        registry: &Arc<RwLock<DocumentRegistry>>,
        document_id: u64,
    ) -> SearchIndexStamp {
        let guard = registry.read().unwrap();
        guard.get(document_id).unwrap().search_index_stamp()
    }

    #[test]
    fn rebuild_searches_existing_content() {
        let (registry, document_id) = make_registry(vec![
            vec![s("apple"), s("banana")],
            vec![s("cherry"), s("durian")],
        ]);
        run_rebuild(
            document_id,
            0,
            current_stamp(&registry, document_id),
            &registry,
        );

        assert_eq!(rows_of(&registry, document_id, "apple"), vec![(0, 0)]);
        assert_eq!(rows_of(&registry, document_id, "durian"), vec![(1, 1)]);
    }

    #[test]
    fn incremental_update_replaces_old_value() {
        let (registry, document_id) = make_registry(vec![vec![s("apple"), s("banana")]]);
        run_rebuild(
            document_id,
            0,
            current_stamp(&registry, document_id),
            &registry,
        );
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
            &[IndexJob::UpdateCell {
                document_id,
                sheet_index: 0,
                stamp: current_stamp(&registry, document_id),
                row: 0,
                col: 0,
                new_text: "orange".to_string(),
                registry: registry.clone(),
            }],
        );

        assert!(ok);
        assert_eq!(
            search_source(&registry, document_id, "orange"),
            SearchSource::Index
        );
        assert!(rows_of(&registry, document_id, "apple").is_empty());
        assert_eq!(rows_of(&registry, document_id, "orange"), vec![(0, 0)]);
        assert_eq!(rows_of(&registry, document_id, "banana"), vec![(0, 1)]);
    }

    #[test]
    fn edited_sheet_uses_scan_fallback_until_incremental_index_commits() {
        let (registry, document_id) = make_registry(vec![vec![s("old")]]);
        run_rebuild(
            document_id,
            0,
            current_stamp(&registry, document_id),
            &registry,
        );
        assert_eq!(
            search_source(&registry, document_id, "old"),
            SearchSource::Index
        );

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

        assert_eq!(
            search_source(&registry, document_id, "new"),
            SearchSource::ScanFallback
        );
        assert!(rows_of(&registry, document_id, "old").is_empty());
        assert_eq!(rows_of(&registry, document_id, "new"), vec![(0, 0)]);

        let ok = run_incremental(
            document_id,
            0,
            &registry,
            &[IndexJob::UpdateCell {
                document_id,
                sheet_index: 0,
                stamp,
                row: 0,
                col: 0,
                new_text: "new".to_string(),
                registry: registry.clone(),
            }],
        );

        assert!(ok);
        assert_eq!(
            search_source(&registry, document_id, "new"),
            SearchSource::Index
        );
        assert!(rows_of(&registry, document_id, "old").is_empty());
        assert_eq!(rows_of(&registry, document_id, "new"), vec![(0, 0)]);
    }

    #[test]
    fn stale_index_search_falls_back_to_current_rows() {
        let (registry, document_id) = make_registry(vec![vec![s("apple")]]);
        run_rebuild(
            document_id,
            0,
            current_stamp(&registry, document_id),
            &registry,
        );
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

        {
            let guard = registry.read().unwrap();
            let editor = guard.get(document_id).unwrap();
            assert_eq!(
                editor.search_sheet(0, "orange", 10).source,
                SearchSource::ScanFallback
            );
        }
        assert!(rows_of(&registry, document_id, "apple").is_empty());
        assert_eq!(rows_of(&registry, document_id, "orange"), vec![(0, 0)]);

        run_rebuild(
            document_id,
            0,
            current_stamp(&registry, document_id),
            &registry,
        );
        assert!(rows_of(&registry, document_id, "apple").is_empty());
        assert_eq!(rows_of(&registry, document_id, "orange"), vec![(0, 0)]);
    }

    #[test]
    fn stale_rebuild_job_does_not_write_into_replaced_active_document() {
        let (registry, old_document_id) = make_registry(vec![vec![s("old")]]);
        let old_stamp = current_stamp(&registry, old_document_id);

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
        let new_document_id = new_editor.document_id();
        {
            let mut guard = registry.write().unwrap();
            guard.replace_active(new_editor);
        }

        run_rebuild(old_document_id, 0, old_stamp, &registry);

        assert_eq!(
            search_source(&registry, new_document_id, "new"),
            SearchSource::ScanFallback
        );
        assert_eq!(rows_of(&registry, new_document_id, "new"), vec![(0, 0)]);
        assert!(registry.read().unwrap().get(old_document_id).is_none());
    }
}
