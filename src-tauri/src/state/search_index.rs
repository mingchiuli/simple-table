use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value};
use tantivy::tokenizer::TextAnalyzer;
use tantivy::{Index, IndexWriter, TantivyDocument, Term, doc};
use tantivy_jieba::JiebaTokenizer;

use crate::types::{CellPosition, CellValue};

const WRITER_ARENA_BYTES: usize = 15_000_000;

struct SchemaFields {
    text: Field,
    row: Field,
    col: Field,
    cell_id: Field,
}

pub struct SearchSheetIndex {
    index: Index,
    schema: Schema,
    text_field: Field,
    cell_id_field: Field,
    writer: Arc<Mutex<IndexWriter>>,
}

struct SearchSheetIndexEntry {
    revision: u64,
    index: SearchSheetIndex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SearchIndexStamp {
    pub generation: u64,
    pub revision: u64,
}

impl Default for SearchIndexStamp {
    fn default() -> Self {
        Self {
            generation: 0,
            revision: 0,
        }
    }
}

pub struct SearchWriterHandle {
    pub writer: Arc<Mutex<IndexWriter>>,
    pub text_field: Field,
    pub row_field: Field,
    pub col_field: Field,
    pub cell_id_field: Field,
}

static NEXT_SEARCH_INDEX_GENERATION: AtomicU64 = AtomicU64::new(1);

pub struct SearchIndexStore {
    generation: u64,
    revision: u64,
    sheets: Vec<Option<SearchSheetIndexEntry>>,
}

impl Default for SearchIndexStore {
    fn default() -> Self {
        Self {
            generation: NEXT_SEARCH_INDEX_GENERATION.fetch_add(1, Ordering::Relaxed),
            revision: 0,
            sheets: Vec::new(),
        }
    }
}

impl SearchIndexStore {
    pub fn stamp(&self) -> SearchIndexStamp {
        SearchIndexStamp {
            generation: self.generation,
            revision: self.revision,
        }
    }

    pub fn mark_stale(&mut self) -> SearchIndexStamp {
        self.revision = self.revision.wrapping_add(1);
        self.stamp()
    }

    pub fn install_sheet_index(
        &mut self,
        sheet_index: usize,
        stamp: SearchIndexStamp,
        index: Option<SearchSheetIndex>,
    ) {
        if stamp != self.stamp() {
            return;
        }
        if self.sheets.len() <= sheet_index {
            self.sheets.resize_with(sheet_index + 1, || None);
        }
        self.sheets[sheet_index] = index.map(|index| SearchSheetIndexEntry {
            revision: stamp.revision,
            index,
        });
    }

    pub fn truncate(&mut self, sheet_count: usize) {
        self.sheets.truncate(sheet_count);
    }

    pub fn writer_handle(
        &self,
        sheet_index: usize,
        stamp: SearchIndexStamp,
    ) -> Option<SearchWriterHandle> {
        if stamp != self.stamp() {
            return None;
        }
        let entry = self.sheets.get(sheet_index)?.as_ref()?;
        if entry.revision != stamp.revision {
            return None;
        }
        let row_field = entry.index.schema.get_field("row").ok()?;
        let col_field = entry.index.schema.get_field("col").ok()?;
        Some(SearchWriterHandle {
            writer: entry.index.writer.clone(),
            text_field: entry.index.text_field,
            row_field,
            col_field,
            cell_id_field: entry.index.cell_id_field,
        })
    }

    pub fn search_sheet(
        &self,
        sheet_index: usize,
        query: &str,
        limit: usize,
    ) -> Option<Vec<CellPosition>> {
        let query = query.trim();
        if query.is_empty() {
            return Some(vec![]);
        }

        let entry = self.sheets.get(sheet_index)?.as_ref()?;
        if entry.revision != self.revision {
            return None;
        };

        Some(search_index(&entry.index, query, limit))
    }
}

pub fn build_sheet_index(rows: &[Vec<CellValue>]) -> Option<SearchSheetIndex> {
    let (index, schema, fields) = match create_tantivy_index() {
        Ok(index) => index,
        Err(error) => {
            eprintln!("Failed to create tantivy index: {error:?}");
            return None;
        }
    };

    let mut writer = match index.writer(WRITER_ARENA_BYTES) {
        Ok(writer) => writer,
        Err(error) => {
            eprintln!("Failed to create index writer: {error:?}");
            return None;
        }
    };

    for (row_idx, row) in rows.iter().enumerate() {
        for (col_idx, cell) in row.iter().enumerate() {
            let text = cell.to_display_string();
            if !text.is_empty()
                && let Err(error) = writer.add_document(doc!(
                    fields.text => text,
                    fields.row => row_idx as u64,
                    fields.col => col_idx as u64,
                    fields.cell_id => format!("{}:{}", row_idx, col_idx),
                ))
            {
                eprintln!("Failed to add document: {error:?}");
            }
        }
    }

    if let Err(error) = writer.commit() {
        eprintln!("Failed to commit index: {error:?}");
        return None;
    }

    Some(SearchSheetIndex {
        index,
        schema,
        text_field: fields.text,
        cell_id_field: fields.cell_id,
        writer: Arc::new(Mutex::new(writer)),
    })
}

fn create_tantivy_index() -> Result<(Index, Schema, SchemaFields), tantivy::TantivyError> {
    let mut schema_builder = Schema::builder();

    let text_field = schema_builder.add_text_field(
        "text",
        TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("jieba")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        ),
    );
    let row_field =
        schema_builder.add_u64_field("row", tantivy::schema::FAST | tantivy::schema::STORED);
    let col_field =
        schema_builder.add_u64_field("col", tantivy::schema::FAST | tantivy::schema::STORED);
    let cell_id_field = schema_builder.add_text_field(
        "cell_id",
        TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("raw")
                .set_index_option(IndexRecordOption::Basic),
        ),
    );

    let schema = schema_builder.build();
    let index = Index::create_in_ram(schema.clone());
    let analyzer = TextAnalyzer::builder(JiebaTokenizer::new()).build();
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

fn search_index(index: &SearchSheetIndex, query: &str, limit: usize) -> Vec<CellPosition> {
    let row_field = match index.schema.get_field("row") {
        Ok(field) => field,
        Err(_) => return vec![],
    };
    let col_field = match index.schema.get_field("col") {
        Ok(field) => field,
        Err(_) => return vec![],
    };
    let reader = match index.index.reader() {
        Ok(reader) => reader,
        Err(error) => {
            eprintln!("Failed to create reader: {error:?}");
            return vec![];
        }
    };
    let searcher = reader.searcher();
    let query_parser = QueryParser::for_index(&index.index, vec![index.text_field]);

    let top_docs = match query_parser.parse_query(query) {
        Ok(query) => match searcher.search(&query, &TopDocs::with_limit(limit).order_by_score()) {
            Ok(docs) => docs,
            Err(error) => {
                eprintln!("Search failed: {error:?}");
                return vec![];
            }
        },
        Err(_) => {
            let term = Term::from_field_text(index.text_field, &query.to_lowercase());
            let term_query = tantivy::query::TermQuery::new(term, IndexRecordOption::Basic);
            match searcher.search(&term_query, &TopDocs::with_limit(limit).order_by_score()) {
                Ok(docs) => docs,
                Err(error) => {
                    eprintln!("Term search failed: {error:?}");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_indexes_are_not_used_until_matching_replacement_installs() {
        let rows = vec![vec![CellValue::String("indexed text".to_string())]];
        let index = build_sheet_index(&rows).expect("index");
        let mut store = SearchIndexStore::default();
        let original_stamp = store.stamp();

        store.install_sheet_index(0, original_stamp, Some(index));
        assert_eq!(
            store.search_sheet(0, "indexed", 10),
            Some(vec![CellPosition { row: 0, col: 0 }])
        );

        let stale_stamp = store.mark_stale();
        assert_eq!(store.search_sheet(0, "indexed", 10), None);

        let stale_index = build_sheet_index(&rows).expect("stale index");
        store.install_sheet_index(0, original_stamp, Some(stale_index));
        assert_eq!(store.search_sheet(0, "indexed", 10), None);

        let replacement_index = build_sheet_index(&rows).expect("replacement index");
        store.install_sheet_index(0, stale_stamp, Some(replacement_index));
        assert_eq!(
            store.search_sheet(0, "indexed", 10),
            Some(vec![CellPosition { row: 0, col: 0 }])
        );
    }
}
