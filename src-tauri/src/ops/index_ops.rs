use std::sync::Arc;
use std::sync::RwLock;

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value};
use tantivy::tokenizer::TextAnalyzer;
use tantivy::{doc, Index, IndexWriter, TantivyDocument, Term};
use tantivy_jieba::JiebaTokenizer;

use crate::state::editor_state::EditorState;
use crate::types::{CellPosition, CellValue, SheetData};

/// 创建 Tantivy 索引
pub fn create_tantivy_index() -> Result<(Index, Schema, tantivy::schema::Field, tantivy::schema::Field, tantivy::schema::Field), tantivy::TantivyError> {
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
    let row_field = schema_builder.add_u64_field("row", tantivy::schema::FAST | tantivy::schema::STORED);
    // 列号字段
    let col_field = schema_builder.add_u64_field("col", tantivy::schema::FAST | tantivy::schema::STORED);

    let schema = schema_builder.build();

    // 创建内存索引
    let index = Index::create_in_ram(schema.clone());

    // 注册 jieba 分词器
    let tokenizer = JiebaTokenizer::new();
    let analyzer = TextAnalyzer::builder(tokenizer).build();
    index.tokenizers().register("jieba", analyzer);

    Ok((index, schema, text_field, row_field, col_field))
}

/// 将单元格值转换为字符串
fn cell_to_string(cell: &CellValue) -> String {
    match cell {
        CellValue::Null => String::new(),
        CellValue::String(s) => s.clone(),
        CellValue::Number(n) => n.to_string(),
        CellValue::Boolean(b) => b.to_string(),
    }
}

/// 重建单个 sheet 的索引
pub fn rebuild_sheet_index(sheet: &mut SheetData) {
    let result = create_tantivy_index();
    if let Err(e) = result {
        eprintln!("Failed to create tantivy index: {:?}", e);
        return;
    }

    let (index, schema, text_field, row_field, col_field) = result.unwrap();

    let mut index_writer: IndexWriter = match index.writer(50_000_000) {
        Ok(writer) => writer,
        Err(e) => {
            eprintln!("Failed to create index writer: {:?}", e);
            return;
        }
    };

    // 清空现有索引
    if let Err(e) = index_writer.delete_all_documents() {
        eprintln!("Failed to delete documents: {:?}", e);
        return;
    }

    // 索引所有单元格
    for (row_idx, row) in sheet.rows.iter().enumerate() {
        for (col_idx, cell) in row.iter().enumerate() {
            let text = cell_to_string(cell);
            if !text.is_empty() {
                if let Err(e) = index_writer.add_document(doc!(
                    text_field => text,
                    row_field => row_idx as u64,
                    col_field => col_idx as u64,
                )) {
                    eprintln!("Failed to add document: {:?}", e);
                }
            }
        }
    }

    if let Err(e) = index_writer.commit() {
        eprintln!("Failed to commit index: {:?}", e);
        return;
    }

    sheet.index.search_index = Some(index);
    sheet.index.search_schema = Some(schema);
    sheet.index.text_field = Some(text_field);
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
        Ok(q) => {
            match searcher.search(&q, &TopDocs::with_limit(limit).order_by_score()) {
                Ok(docs) => docs,
                Err(e) => {
                    eprintln!("Search failed: {:?}", e);
                    return vec![];
                }
            }
        }
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
        if let Ok(doc) = searcher.doc::<TantivyDocument>(doc_address) {
            if let (Some(row_val), Some(col_val)) = (doc.get_first(row_field), doc.get_first(col_field)) {
                if let (Some(row), Some(col)) = (row_val.as_u64(), col_val.as_u64()) {
                    results.push(CellPosition {
                        row: row as usize,
                        col: col as usize,
                    });
                }
            }
        }
    }

    results
}

/// 异步重建指定 sheet 的索引
pub fn spawn_rebuild_sheet_index(sheet_index: usize, state: Arc<RwLock<Option<EditorState>>>) {
    std::thread::spawn(move || {
        if let Ok(mut guard) = state.write() {
            if let Some(ref mut editor_state) = *guard {
                if let Some(sheet) = editor_state.file_data.sheets.get_mut(sheet_index) {
                    rebuild_sheet_index(sheet);
                }
            }
        }
    });
}

/// 异步重建所有 sheets 的索引
pub fn spawn_rebuild_all_sheets_index(state: Arc<RwLock<Option<EditorState>>>) {
    std::thread::spawn(move || {
        if let Ok(mut guard) = state.write() {
            if let Some(ref mut editor_state) = *guard {
                for sheet in &mut editor_state.file_data.sheets {
                    rebuild_sheet_index(sheet);
                }
            }
        }
    });
}
