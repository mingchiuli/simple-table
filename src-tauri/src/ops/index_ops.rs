use std::collections::HashMap;
#[cfg(test)]
use std::mem;
use std::sync::{Arc, Mutex, OnceLock, RwLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value};
use tantivy::tokenizer::TextAnalyzer;
use tantivy::{Index, IndexWriter, TantivyDocument, Term, doc};
use tantivy_jieba::JiebaTokenizer;

use crate::state::editor_state::EditorState;
use crate::types::{CellPosition, CellValue, SheetData};

/// 单个 sheet 的索引 writer arena 大小（增量编辑场景下不需要太大）
const WRITER_ARENA_BYTES: usize = 15_000_000;

/// Tantivy schema 字段集合
struct SchemaFields {
    text: Field,
    row: Field,
    col: Field,
    /// 单元格主键，"row:col"，未分词
    cell_id: Field,
}

/// 创建 Tantivy 索引
fn create_tantivy_index() -> Result<(Index, Schema, SchemaFields), tantivy::TantivyError> {
    let mut schema_builder = Schema::builder();

    // 文本内容字段 - 使用 jieba 分词
    let text_field = schema_builder.add_text_field(
        "text",
        TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("jieba")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored(),
    );

    // 行号字段
    let row_field =
        schema_builder.add_u64_field("row", tantivy::schema::FAST | tantivy::schema::STORED);
    // 列号字段
    let col_field =
        schema_builder.add_u64_field("col", tantivy::schema::FAST | tantivy::schema::STORED);
    // 单元格主键字段（"raw" 分词器，不切分；用于 delete_term）
    let cell_id_field = schema_builder.add_text_field(
        "cell_id",
        TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("raw")
                .set_index_option(IndexRecordOption::Basic),
        ),
    );

    let schema = schema_builder.build();

    // 创建内存索引
    let index = Index::create_in_ram(schema.clone());

    // 注册 jieba 分词器
    let tokenizer = JiebaTokenizer::new();
    let analyzer = TextAnalyzer::builder(tokenizer).build();
    index.tokenizers().register("jieba", analyzer);

    Ok((
        index,
        schema,
        SchemaFields {
            text: text_field,
            row: row_field,
            col: col_field,
            cell_id: cell_id_field,
        },
    ))
}

/// 索引构建产物
struct BuiltIndex {
    index: Index,
    schema: Schema,
    text_field: Field,
    cell_id_field: Field,
    writer: Arc<Mutex<IndexWriter>>,
}

/// 基于行数据构建一个新的索引并保留 writer 句柄
fn build_index_from_rows(rows: &[Vec<CellValue>]) -> Option<BuiltIndex> {
    let (index, schema, fields) = match create_tantivy_index() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to create tantivy index: {:?}", e);
            return None;
        }
    };

    let mut writer: IndexWriter = match index.writer(WRITER_ARENA_BYTES) {
        Ok(writer) => writer,
        Err(e) => {
            eprintln!("Failed to create index writer: {:?}", e);
            return None;
        }
    };

    for (row_idx, row) in rows.iter().enumerate() {
        for (col_idx, cell) in row.iter().enumerate() {
            let text = cell.to_display_string();
            if !text.is_empty()
                && let Err(e) = writer.add_document(doc!(
                    fields.text => text,
                    fields.row => row_idx as u64,
                    fields.col => col_idx as u64,
                    fields.cell_id => format!("{}:{}", row_idx, col_idx),
                ))
            {
                eprintln!("Failed to add document: {:?}", e);
            }
        }
    }

    if let Err(e) = writer.commit() {
        eprintln!("Failed to commit index: {:?}", e);
        return None;
    }

    Some(BuiltIndex {
        index,
        schema,
        text_field: fields.text,
        cell_id_field: fields.cell_id,
        writer: Arc::new(Mutex::new(writer)),
    })
}

fn install_built_index(sheet: &mut SheetData, built: BuiltIndex) {
    sheet.index.search_index = Some(built.index);
    sheet.index.search_schema = Some(built.schema);
    sheet.index.text_field = Some(built.text_field);
    sheet.index.cell_id_field = Some(built.cell_id_field);
    sheet.index.writer = Some(built.writer);
}

/// 同步重建单个 sheet 的索引（仅在已持有 sheet 可变借用时使用，例如初始化场景）
#[allow(dead_code)]
pub fn rebuild_sheet_index(sheet: &mut SheetData) {
    if let Some(built) = build_index_from_rows(&sheet.rows) {
        install_built_index(sheet, built);
    }
}

/// 搜索单元格位置
pub fn search_cells(sheet: &SheetData, query: &str, limit: usize) -> Vec<CellPosition> {
    let query = query.trim();
    if query.is_empty() {
        return vec![];
    }

    let index = match &sheet.index.search_index {
        Some(idx) => idx,
        None => return vec![],
    };

    let text_field = match sheet.index.text_field {
        Some(field) => field,
        None => return vec![],
    };

    let schema = match &sheet.index.search_schema {
        Some(s) => s,
        None => return vec![],
    };

    let row_field = match schema.get_field("row") {
        Ok(f) => f,
        Err(_) => return vec![],
    };

    let col_field = match schema.get_field("col") {
        Ok(f) => f,
        Err(_) => return vec![],
    };

    let reader = match index.reader() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to create reader: {:?}", e);
            return vec![];
        }
    };
    let searcher = reader.searcher();

    let query_parser = QueryParser::for_index(index, vec![text_field]);

    // 解析查询
    let parsed_query = query_parser.parse_query(query);

    let top_docs = match parsed_query {
        Ok(q) => match searcher.search(&q, &TopDocs::with_limit(limit).order_by_score()) {
            Ok(docs) => docs,
            Err(e) => {
                eprintln!("Search failed: {:?}", e);
                return vec![];
            }
        },
        Err(_) => {
            // 如果解析失败，尝试作为词项查询
            let term = Term::from_field_text(text_field, &query.to_lowercase());
            let term_query = tantivy::query::TermQuery::new(term, IndexRecordOption::Basic);
            match searcher.search(&term_query, &TopDocs::with_limit(limit).order_by_score()) {
                Ok(docs) => docs,
                Err(e) => {
                    eprintln!("Term search failed: {:?}", e);
                    return vec![];
                }
            }
        }
    };

    let mut results = Vec::new();
    for (_score, doc_address) in top_docs {
        if let Ok(doc) = searcher.doc::<TantivyDocument>(doc_address)
            && let (Some(row_val), Some(col_val)) =
                (doc.get_first(row_field), doc.get_first(col_field))
            && let (Some(row), Some(col)) = (row_val.as_u64(), col_val.as_u64())
        {
            results.push(CellPosition {
                row: row as usize,
                col: col as usize,
            });
        }
    }

    results
}

/// 索引更新作业
enum IndexJob {
    /// 全量重建（兜底；适用于排序、中间插入/删除等结构改变操作）
    Rebuild {
        sheet_index: usize,
        state: Arc<RwLock<Option<EditorState>>>,
    },
    /// 单格更新：删除旧文档 + 添加新文档
    #[allow(dead_code)]
    UpdateCell {
        sheet_index: usize,
        row: usize,
        col: usize,
        new_text: String,
        state: Arc<RwLock<Option<EditorState>>>,
    },
    /// 末尾追加一行（仅写入非空单元格；全空时为 no-op）
    #[allow(dead_code)]
    AppendRow {
        sheet_index: usize,
        row_index: usize,
        row_data: Vec<CellValue>,
        state: Arc<RwLock<Option<EditorState>>>,
    },
    /// 末尾追加一列（仅写入非空单元格；全空时为 no-op）
    #[allow(dead_code)]
    AppendColumn {
        sheet_index: usize,
        col_index: usize,
        col_data: Vec<CellValue>,
        state: Arc<RwLock<Option<EditorState>>>,
    },
    /// 删除末尾一行（按 cell_id term 删除该行所有 doc）
    #[allow(dead_code)]
    DeleteLastRow {
        sheet_index: usize,
        row_index: usize,
        col_count: usize,
        state: Arc<RwLock<Option<EditorState>>>,
    },
    /// 删除末尾一列（按 cell_id term 删除该列所有 doc）
    #[allow(dead_code)]
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

/// 同一 sheet 在去抖窗口内的待处理作业
struct SheetPending {
    /// 是否需要全量重建（一旦置位即丢弃所有增量作业）
    rebuild: bool,
    /// 增量作业按到达顺序保留，回放到 writer 上
    incremental: Vec<IndexJob>,
    /// 任意一个作业的 state Arc
    state: Arc<RwLock<Option<EditorState>>>,
}

/// 索引去抖延迟：连续编辑时合并多次更新请求
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

/// 单后台线程：阻塞等待任务，按 sheet 去重并支持增量/全量两条路径
fn index_worker(rx: mpsc::Receiver<IndexJob>) {
    loop {
        let first = match rx.recv() {
            Ok(job) => job,
            Err(_) => return,
        };

        let mut pending: HashMap<usize, SheetPending> = HashMap::new();
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

        for (sheet_index, p) in pending {
            if p.rebuild {
                run_rebuild(sheet_index, &p.state);
            } else if !p.incremental.is_empty()
                && !run_incremental(sheet_index, &p.state, &p.incremental)
            {
                // 增量路径失败（writer 缺失或 commit 出错）→ 退化为全量重建
                run_rebuild(sheet_index, &p.state);
            }
        }
    }
}

/// sheet 写入路径所需的字段句柄与 writer
struct WriterHandle {
    writer: Arc<Mutex<IndexWriter>>,
    text_field: Field,
    row_field: Field,
    col_field: Field,
    cell_id_field: Field,
}

/// 短暂 read 锁，提取目标 sheet 的 writer 与字段句柄
fn snapshot_writer_handle(
    state: &Arc<RwLock<Option<EditorState>>>,
    sheet_index: usize,
) -> Option<WriterHandle> {
    let guard = state.read().ok()?;
    let editor = guard.as_ref()?;
    let sheet = editor.file_data().sheets.get(sheet_index)?;
    let writer = sheet.index.writer.clone()?;
    let schema = sheet.index.search_schema.as_ref()?;
    let text_field = sheet.index.text_field?;
    let cell_id_field = sheet.index.cell_id_field?;
    let row_field = schema.get_field("row").ok()?;
    let col_field = schema.get_field("col").ok()?;
    Some(WriterHandle {
        writer,
        text_field,
        row_field,
        col_field,
        cell_id_field,
    })
}

/// 全量重建：锁外构建 + 短写锁安装
fn run_rebuild(sheet_index: usize, state: &Arc<RwLock<Option<EditorState>>>) {
    let rows_snapshot = match state.read() {
        Ok(guard) => guard.as_ref().and_then(|s| {
            s.file_data()
                .sheets
                .get(sheet_index)
                .map(|sh| sh.rows.clone())
        }),
        Err(_) => None,
    };
    let Some(rows) = rows_snapshot else { return };
    let Some(built) = build_index_from_rows(&rows) else {
        return;
    };

    if let Ok(mut guard) = state.write()
        && let Some(editor_state) = guard.as_mut()
        && let Some(sheet) = editor_state.sheet_mut_for_indexing(sheet_index)
    {
        install_built_index(sheet, built);
    }
}

/// 增量路径：复用 sheet 持有的 writer，批量执行 ops 后一次性 commit
fn run_incremental(
    sheet_index: usize,
    state: &Arc<RwLock<Option<EditorState>>>,
    ops: &[IndexJob],
) -> bool {
    let Some(handle) = snapshot_writer_handle(state, sheet_index) else {
        return false;
    };
    let mut writer = match handle.writer.lock() {
        Ok(w) => w,
        Err(_) => return false,
    };

    for op in ops {
        match op {
            IndexJob::UpdateCell {
                row, col, new_text, ..
            } => {
                let cell_id = format!("{}:{}", row, col);
                let term = Term::from_field_text(handle.cell_id_field, &cell_id);
                writer.delete_term(term);
                if !new_text.is_empty()
                    && let Err(e) = writer.add_document(doc!(
                        handle.text_field => new_text.clone(),
                        handle.row_field => *row as u64,
                        handle.col_field => *col as u64,
                        handle.cell_id_field => cell_id,
                    ))
                {
                    eprintln!("incremental add_document failed: {:?}", e);
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
                    if let Err(e) = writer.add_document(doc!(
                        handle.text_field => text,
                        handle.row_field => *row_index as u64,
                        handle.col_field => col_idx as u64,
                        handle.cell_id_field => format!("{}:{}", row_index, col_idx),
                    )) {
                        eprintln!("incremental append row failed: {:?}", e);
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
                    if let Err(e) = writer.add_document(doc!(
                        handle.text_field => text,
                        handle.row_field => row_idx as u64,
                        handle.col_field => *col_index as u64,
                        handle.cell_id_field => format!("{}:{}", row_idx, col_index),
                    )) {
                        eprintln!("incremental append column failed: {:?}", e);
                        return false;
                    }
                }
            }
            IndexJob::DeleteLastRow {
                row_index,
                col_count,
                ..
            } => {
                for c in 0..*col_count {
                    let term = Term::from_field_text(
                        handle.cell_id_field,
                        &format!("{}:{}", row_index, c),
                    );
                    writer.delete_term(term);
                }
            }
            IndexJob::DeleteLastColumn {
                col_index,
                row_count,
                ..
            } => {
                for r in 0..*row_count {
                    let term = Term::from_field_text(
                        handle.cell_id_field,
                        &format!("{}:{}", r, col_index),
                    );
                    writer.delete_term(term);
                }
            }
            IndexJob::Rebuild { .. } => unreachable!("rebuild handled in dispatcher"),
        }
    }

    if let Err(e) = writer.commit() {
        eprintln!("incremental commit failed: {:?}", e);
        return false;
    }
    true
}

/// 投递全量重建作业（结构性改变、首次构建、增量回退）
#[allow(dead_code)]
pub fn spawn_rebuild_sheet_index(sheet_index: usize, state: Arc<RwLock<Option<EditorState>>>) {
    let _ = index_queue().send(IndexJob::Rebuild { sheet_index, state });
}

/// 投递所有 sheet 的全量重建作业
pub fn spawn_rebuild_all_sheets_index(state: Arc<RwLock<Option<EditorState>>>) {
    let count = match state.read() {
        Ok(guard) => guard
            .as_ref()
            .map(|s| s.file_data().sheets.len())
            .unwrap_or(0),
        Err(_) => 0,
    };
    let queue = index_queue();
    for i in 0..count {
        let _ = queue.send(IndexJob::Rebuild {
            sheet_index: i,
            state: state.clone(),
        });
    }
}

/// 单格更新（增量）
#[allow(dead_code)]
pub fn spawn_update_cell_index(
    sheet_index: usize,
    row: usize,
    col: usize,
    new_value: &CellValue,
    state: Arc<RwLock<Option<EditorState>>>,
) {
    let new_text = new_value.to_display_string();
    let _ = index_queue().send(IndexJob::UpdateCell {
        sheet_index,
        row,
        col,
        new_text,
        state,
    });
}

/// 末尾追加一行（增量；row_data 全空时也会投递，worker 内部跳过空格）
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

/// 末尾追加一列（增量；col_data 全空时也会投递，worker 内部跳过空格）
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

/// 删除末尾一行（增量）
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

/// 删除末尾一列（增量）
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
    use crate::types::SheetData;

    fn s(v: &str) -> CellValue {
        CellValue::String(v.to_string())
    }

    fn make_sheet(rows: Vec<Vec<CellValue>>) -> SheetData {
        let mut sheet = SheetData {
            name: "Test".into(),
            rows,
            ..Default::default()
        };
        rebuild_sheet_index(&mut sheet);
        sheet
    }

    /// 复用 worker 同款逻辑，但直接同步在当前线程上运行。
    /// 仅供测试使用：构造一个临时 EditorState 包装并调用 run_incremental。
    fn apply_incremental_sync(sheet: &mut SheetData, ops: Vec<IndexJob>) -> bool {
        use crate::types::FileData;
        let editor = EditorState::with_workbook(
            FileData {
                path: String::new(),
                file_name: String::new(),
                sheets: vec![mem::take(sheet)],
            },
            None,
        );
        let state = Arc::new(RwLock::new(Some(editor)));
        let ok = run_incremental(0, &state, &ops);
        // 取回 sheet
        let mut guard = state.write().unwrap();
        let editor = guard.as_mut().unwrap();
        mem::swap(sheet, editor.sheet_mut_for_indexing(0).unwrap());
        ok
    }

    fn rows_of(positions: &[CellPosition]) -> Vec<(usize, usize)> {
        let mut v: Vec<_> = positions.iter().map(|p| (p.row, p.col)).collect();
        v.sort();
        v
    }

    #[test]
    fn search_finds_existing_content() {
        let sheet = make_sheet(vec![
            vec![s("apple"), s("banana")],
            vec![s("cherry"), s("durian")],
        ]);
        assert_eq!(rows_of(&search_cells(&sheet, "apple", 10)), vec![(0, 0)]);
        assert_eq!(rows_of(&search_cells(&sheet, "durian", 10)), vec![(1, 1)]);
    }

    #[test]
    fn update_cell_replaces_old_value_in_index() {
        let mut sheet = make_sheet(vec![vec![s("apple"), s("banana")]]);
        sheet.rows[0][0] = s("orange");
        let state_arc: Arc<RwLock<Option<EditorState>>> = Arc::new(RwLock::new(None));
        let job = IndexJob::UpdateCell {
            sheet_index: 0,
            row: 0,
            col: 0,
            new_text: "orange".into(),
            state: state_arc,
        };
        assert!(apply_incremental_sync(&mut sheet, vec![job]));
        assert!(search_cells(&sheet, "apple", 10).is_empty());
        assert_eq!(rows_of(&search_cells(&sheet, "orange", 10)), vec![(0, 0)]);
        assert_eq!(rows_of(&search_cells(&sheet, "banana", 10)), vec![(0, 1)]);
    }

    #[test]
    fn update_cell_to_empty_removes_from_index() {
        let mut sheet = make_sheet(vec![vec![s("apple")]]);
        sheet.rows[0][0] = CellValue::Null;
        let state_arc: Arc<RwLock<Option<EditorState>>> = Arc::new(RwLock::new(None));
        let job = IndexJob::UpdateCell {
            sheet_index: 0,
            row: 0,
            col: 0,
            new_text: String::new(),
            state: state_arc,
        };
        assert!(apply_incremental_sync(&mut sheet, vec![job]));
        assert!(search_cells(&sheet, "apple", 10).is_empty());
    }

    #[test]
    fn append_row_with_content_adds_documents() {
        let mut sheet = make_sheet(vec![vec![s("apple"), s("banana")]]);
        sheet.rows.push(vec![s("orange"), CellValue::Null]);
        let state_arc: Arc<RwLock<Option<EditorState>>> = Arc::new(RwLock::new(None));
        let job = IndexJob::AppendRow {
            sheet_index: 0,
            row_index: 1,
            row_data: vec![s("orange"), CellValue::Null],
            state: state_arc,
        };
        assert!(apply_incremental_sync(&mut sheet, vec![job]));
        assert_eq!(rows_of(&search_cells(&sheet, "orange", 10)), vec![(1, 0)]);
        assert_eq!(rows_of(&search_cells(&sheet, "apple", 10)), vec![(0, 0)]);
    }

    #[test]
    fn append_row_all_null_is_noop() {
        let mut sheet = make_sheet(vec![vec![s("apple")]]);
        sheet.rows.push(vec![CellValue::Null]);
        let state_arc: Arc<RwLock<Option<EditorState>>> = Arc::new(RwLock::new(None));
        let job = IndexJob::AppendRow {
            sheet_index: 0,
            row_index: 1,
            row_data: vec![CellValue::Null],
            state: state_arc,
        };
        assert!(apply_incremental_sync(&mut sheet, vec![job]));
        // 仍能搜到旧内容，且新行无文档
        assert_eq!(rows_of(&search_cells(&sheet, "apple", 10)), vec![(0, 0)]);
    }

    #[test]
    fn append_column_with_content_adds_documents() {
        let mut sheet = make_sheet(vec![vec![s("apple")], vec![s("cherry")]]);
        for (i, row) in sheet.rows.iter_mut().enumerate() {
            row.push(if i == 0 { s("kiwi") } else { CellValue::Null });
        }
        let state_arc: Arc<RwLock<Option<EditorState>>> = Arc::new(RwLock::new(None));
        let job = IndexJob::AppendColumn {
            sheet_index: 0,
            col_index: 1,
            col_data: vec![s("kiwi"), CellValue::Null],
            state: state_arc,
        };
        assert!(apply_incremental_sync(&mut sheet, vec![job]));
        assert_eq!(rows_of(&search_cells(&sheet, "kiwi", 10)), vec![(0, 1)]);
        assert_eq!(rows_of(&search_cells(&sheet, "cherry", 10)), vec![(1, 0)]);
    }

    #[test]
    fn delete_last_row_removes_documents() {
        let mut sheet = make_sheet(vec![
            vec![s("apple"), s("banana")],
            vec![s("orange"), s("kiwi")],
        ]);
        sheet.rows.pop();
        let state_arc: Arc<RwLock<Option<EditorState>>> = Arc::new(RwLock::new(None));
        let job = IndexJob::DeleteLastRow {
            sheet_index: 0,
            row_index: 1,
            col_count: 2,
            state: state_arc,
        };
        assert!(apply_incremental_sync(&mut sheet, vec![job]));
        assert!(search_cells(&sheet, "orange", 10).is_empty());
        assert!(search_cells(&sheet, "kiwi", 10).is_empty());
        assert_eq!(rows_of(&search_cells(&sheet, "apple", 10)), vec![(0, 0)]);
    }

    #[test]
    fn delete_last_column_removes_documents() {
        let mut sheet = make_sheet(vec![
            vec![s("apple"), s("kiwi")],
            vec![s("cherry"), s("mango")],
        ]);
        for row in sheet.rows.iter_mut() {
            row.pop();
        }
        let state_arc: Arc<RwLock<Option<EditorState>>> = Arc::new(RwLock::new(None));
        let job = IndexJob::DeleteLastColumn {
            sheet_index: 0,
            col_index: 1,
            row_count: 2,
            state: state_arc,
        };
        assert!(apply_incremental_sync(&mut sheet, vec![job]));
        assert!(search_cells(&sheet, "kiwi", 10).is_empty());
        assert!(search_cells(&sheet, "mango", 10).is_empty());
        assert_eq!(rows_of(&search_cells(&sheet, "apple", 10)), vec![(0, 0)]);
        assert_eq!(rows_of(&search_cells(&sheet, "cherry", 10)), vec![(1, 0)]);
    }

    #[test]
    fn batched_updates_share_one_commit() {
        let mut sheet = make_sheet(vec![
            vec![s("apple"), s("banana")],
            vec![s("cherry"), s("durian")],
        ]);
        sheet.rows[0][0] = s("orange");
        sheet.rows[1][1] = s("kiwi");
        let state_arc: Arc<RwLock<Option<EditorState>>> = Arc::new(RwLock::new(None));
        let jobs = vec![
            IndexJob::UpdateCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                new_text: "orange".into(),
                state: state_arc.clone(),
            },
            IndexJob::UpdateCell {
                sheet_index: 0,
                row: 1,
                col: 1,
                new_text: "kiwi".into(),
                state: state_arc,
            },
        ];
        assert!(apply_incremental_sync(&mut sheet, jobs));
        assert!(search_cells(&sheet, "apple", 10).is_empty());
        assert!(search_cells(&sheet, "durian", 10).is_empty());
        assert_eq!(rows_of(&search_cells(&sheet, "orange", 10)), vec![(0, 0)]);
        assert_eq!(rows_of(&search_cells(&sheet, "kiwi", 10)), vec![(1, 1)]);
    }

    #[test]
    fn merge_job_rebuild_supersedes_incrementals() {
        let mut pending: HashMap<usize, SheetPending> = HashMap::new();
        let state_arc: Arc<RwLock<Option<EditorState>>> = Arc::new(RwLock::new(None));
        merge_job(
            &mut pending,
            IndexJob::UpdateCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                new_text: "x".into(),
                state: state_arc.clone(),
            },
        );
        merge_job(
            &mut pending,
            IndexJob::Rebuild {
                sheet_index: 0,
                state: state_arc.clone(),
            },
        );
        merge_job(
            &mut pending,
            IndexJob::UpdateCell {
                sheet_index: 0,
                row: 1,
                col: 1,
                new_text: "y".into(),
                state: state_arc,
            },
        );
        let entry = pending.get(&0).unwrap();
        assert!(entry.rebuild);
        assert!(
            entry.incremental.is_empty(),
            "incrementals after rebuild must be dropped"
        );
    }
}
