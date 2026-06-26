use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use tantivy::{Term, doc};

use crate::state::editor_state::EditorState;
use crate::state::search_index::build_sheet_index;
use crate::types::CellValue;

enum IndexJob {
    Rebuild {
        sheet_index: usize,
        state: Arc<RwLock<Option<EditorState>>>,
    },
    UpdateCell {
        sheet_index: usize,
        row: usize,
        col: usize,
        new_text: String,
        state: Arc<RwLock<Option<EditorState>>>,
    },
    AppendRow {
        sheet_index: usize,
        row_index: usize,
        row_data: Vec<CellValue>,
        state: Arc<RwLock<Option<EditorState>>>,
    },
    AppendColumn {
        sheet_index: usize,
        col_index: usize,
        col_data: Vec<CellValue>,
        state: Arc<RwLock<Option<EditorState>>>,
    },
    DeleteLastRow {
        sheet_index: usize,
        row_index: usize,
        col_count: usize,
        state: Arc<RwLock<Option<EditorState>>>,
    },
    DeleteLastColumn {
        sheet_index: usize,
        col_index: usize,
        row_count: usize,
        state: Arc<RwLock<Option<EditorState>>>,
    },
}

impl IndexJob {
    fn sheet_index(&self) -> usize {
        match self {
            IndexJob::Rebuild { sheet_index, .. }
            | IndexJob::UpdateCell { sheet_index, .. }
            | IndexJob::AppendRow { sheet_index, .. }
            | IndexJob::AppendColumn { sheet_index, .. }
            | IndexJob::DeleteLastRow { sheet_index, .. }
            | IndexJob::DeleteLastColumn { sheet_index, .. } => *sheet_index,
        }
    }

    fn state(&self) -> &Arc<RwLock<Option<EditorState>>> {
        match self {
            IndexJob::Rebuild { state, .. }
            | IndexJob::UpdateCell { state, .. }
            | IndexJob::AppendRow { state, .. }
            | IndexJob::AppendColumn { state, .. }
            | IndexJob::DeleteLastRow { state, .. }
            | IndexJob::DeleteLastColumn { state, .. } => state,
        }
    }
}

struct SheetPending {
    rebuild: bool,
    incremental: Vec<IndexJob>,
    state: Arc<RwLock<Option<EditorState>>>,
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

fn merge_job(pending: &mut HashMap<usize, SheetPending>, job: IndexJob) {
    let sheet_index = job.sheet_index();
    let state = job.state().clone();
    let entry = pending.entry(sheet_index).or_insert_with(|| SheetPending {
        rebuild: false,
        incremental: Vec::new(),
        state: state.clone(),
    });
    entry.state = state;
    match job {
        IndexJob::Rebuild { .. } => {
            entry.rebuild = true;
            entry.incremental.clear();
        }
        other => {
            if !entry.rebuild {
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

        for (sheet_index, pending) in pending {
            if pending.rebuild {
                run_rebuild(sheet_index, &pending.state);
            } else if !pending.incremental.is_empty()
                && !run_incremental(sheet_index, &pending.state, &pending.incremental)
            {
                run_rebuild(sheet_index, &pending.state);
            }
        }
    }
}

fn run_rebuild(sheet_index: usize, state: &Arc<RwLock<Option<EditorState>>>) {
    let rows_snapshot = match state.read() {
        Ok(guard) => guard.as_ref().and_then(|editor| {
            editor
                .file_data()
                .sheets
                .get(sheet_index)
                .map(|sheet| sheet.rows.clone())
        }),
        Err(_) => None,
    };
    let Some(rows) = rows_snapshot else { return };
    let built_index = build_sheet_index(&rows);

    if let Ok(mut guard) = state.write()
        && let Some(editor_state) = guard.as_mut()
    {
        editor_state.install_search_index(sheet_index, built_index);
    }
}

fn run_incremental(
    sheet_index: usize,
    state: &Arc<RwLock<Option<EditorState>>>,
    ops: &[IndexJob],
) -> bool {
    let Some(handle) = state
        .read()
        .ok()
        .and_then(|guard| guard.as_ref()?.search_writer_handle(sheet_index))
    else {
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
            IndexJob::AppendRow {
                row_index,
                row_data,
                ..
            } => {
                for (col_idx, cell) in row_data.iter().enumerate() {
                    let text = cell.to_display_string();
                    if text.is_empty() {
                        continue;
                    }
                    if let Err(error) = writer.add_document(doc!(
                        handle.text_field => text,
                        handle.row_field => *row_index as u64,
                        handle.col_field => col_idx as u64,
                        handle.cell_id_field => format!("{}:{}", row_index, col_idx),
                    )) {
                        eprintln!("incremental append row failed: {error:?}");
                        return false;
                    }
                }
            }
            IndexJob::AppendColumn {
                col_index,
                col_data,
                ..
            } => {
                for (row_idx, cell) in col_data.iter().enumerate() {
                    let text = cell.to_display_string();
                    if text.is_empty() {
                        continue;
                    }
                    if let Err(error) = writer.add_document(doc!(
                        handle.text_field => text,
                        handle.row_field => row_idx as u64,
                        handle.col_field => *col_index as u64,
                        handle.cell_id_field => format!("{}:{}", row_idx, col_index),
                    )) {
                        eprintln!("incremental append column failed: {error:?}");
                        return false;
                    }
                }
            }
            IndexJob::DeleteLastRow {
                row_index,
                col_count,
                ..
            } => {
                for col in 0..*col_count {
                    writer.delete_term(Term::from_field_text(
                        handle.cell_id_field,
                        &format!("{}:{}", row_index, col),
                    ));
                }
            }
            IndexJob::DeleteLastColumn {
                col_index,
                row_count,
                ..
            } => {
                for row in 0..*row_count {
                    writer.delete_term(Term::from_field_text(
                        handle.cell_id_field,
                        &format!("{}:{}", row, col_index),
                    ));
                }
            }
            IndexJob::Rebuild { .. } => unreachable!("rebuild handled by dispatcher"),
        }
    }

    if let Err(error) = writer.commit() {
        eprintln!("incremental commit failed: {error:?}");
        return false;
    }
    true
}

pub fn spawn_rebuild_all_sheets_index(state: Arc<RwLock<Option<EditorState>>>) {
    let count = match state.read() {
        Ok(guard) => guard
            .as_ref()
            .map(|editor| editor.file_data().sheets.len())
            .unwrap_or(0),
        Err(_) => 0,
    };

    let queue = index_queue();
    for sheet_index in 0..count {
        let _ = queue.send(IndexJob::Rebuild {
            sheet_index,
            state: state.clone(),
        });
    }
}

pub fn spawn_update_cell_index(
    sheet_index: usize,
    row: usize,
    col: usize,
    new_value: &CellValue,
    state: Arc<RwLock<Option<EditorState>>>,
) {
    let _ = index_queue().send(IndexJob::UpdateCell {
        sheet_index,
        row,
        col,
        new_text: new_value.to_display_string(),
        state,
    });
}

#[allow(dead_code)]
pub fn spawn_append_row_index(
    sheet_index: usize,
    row_index: usize,
    row_data: Vec<CellValue>,
    state: Arc<RwLock<Option<EditorState>>>,
) {
    let _ = index_queue().send(IndexJob::AppendRow {
        sheet_index,
        row_index,
        row_data,
        state,
    });
}

#[allow(dead_code)]
pub fn spawn_append_column_index(
    sheet_index: usize,
    col_index: usize,
    col_data: Vec<CellValue>,
    state: Arc<RwLock<Option<EditorState>>>,
) {
    let _ = index_queue().send(IndexJob::AppendColumn {
        sheet_index,
        col_index,
        col_data,
        state,
    });
}

#[allow(dead_code)]
pub fn spawn_delete_last_row_index(
    sheet_index: usize,
    row_index: usize,
    col_count: usize,
    state: Arc<RwLock<Option<EditorState>>>,
) {
    let _ = index_queue().send(IndexJob::DeleteLastRow {
        sheet_index,
        row_index,
        col_count,
        state,
    });
}

#[allow(dead_code)]
pub fn spawn_delete_last_column_index(
    sheet_index: usize,
    col_index: usize,
    row_count: usize,
    state: Arc<RwLock<Option<EditorState>>>,
) {
    let _ = index_queue().send(IndexJob::DeleteLastColumn {
        sheet_index,
        col_index,
        row_count,
        state,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::Operation;
    use crate::types::{FileData, SheetData};

    fn s(value: &str) -> CellValue {
        CellValue::String(value.to_string())
    }

    fn make_state(rows: Vec<Vec<CellValue>>) -> Arc<RwLock<Option<EditorState>>> {
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
        Arc::new(RwLock::new(Some(editor)))
    }

    fn rows_of(state: &Arc<RwLock<Option<EditorState>>>, query: &str) -> Vec<(usize, usize)> {
        let guard = state.read().unwrap();
        let editor = guard.as_ref().unwrap();
        let mut rows: Vec<_> = editor
            .search_sheet(0, query, 10)
            .iter()
            .map(|position| (position.row, position.col))
            .collect();
        rows.sort();
        rows
    }

    #[test]
    fn rebuild_searches_existing_content() {
        let state = make_state(vec![
            vec![s("apple"), s("banana")],
            vec![s("cherry"), s("durian")],
        ]);
        run_rebuild(0, &state);

        assert_eq!(rows_of(&state, "apple"), vec![(0, 0)]);
        assert_eq!(rows_of(&state, "durian"), vec![(1, 1)]);
    }

    #[test]
    fn incremental_update_replaces_old_value() {
        let state = make_state(vec![vec![s("apple"), s("banana")]]);
        run_rebuild(0, &state);
        {
            let mut guard = state.write().unwrap();
            let editor = guard.as_mut().unwrap();
            editor
                .execute(Operation::SetCell {
                    sheet_index: 0,
                    row: 0,
                    col: 0,
                    old_value: s("apple"),
                    new_value: s("orange"),
                })
                .unwrap();
        }

        let ok = run_incremental(
            0,
            &state,
            &[IndexJob::UpdateCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                new_text: "orange".to_string(),
                state: state.clone(),
            }],
        );

        assert!(ok);
        assert!(rows_of(&state, "apple").is_empty());
        assert_eq!(rows_of(&state, "orange"), vec![(0, 0)]);
        assert_eq!(rows_of(&state, "banana"), vec![(0, 1)]);
    }
}
