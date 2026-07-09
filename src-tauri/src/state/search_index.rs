use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value};
use tantivy::tokenizer::{TextAnalyzer, TokenStream};
use tantivy::{Index, IndexWriter, TantivyDocument, Term, doc};
use tantivy_jieba::JiebaTokenizer;

#[cfg(test)]
use crate::types::CellValue;
use crate::types::SheetData;

const WRITER_ARENA_BYTES: usize = 15_000_000;

struct SchemaFields {
    text: Field,
    display: Field,
    row: Field,
    col: Field,
    cell_id: Field,
}

pub struct SearchSheetIndex {
    index: Index,
    schema: Schema,
    text_field: Field,
    display_field: Field,
    cell_id_field: Field,
    writer: Arc<Mutex<IndexWriter>>,
}

struct SearchSheetIndexEntry {
    revision: u64,
    index: SearchSheetIndex,
}

enum SearchSheetSlot {
    Fresh(SearchSheetIndexEntry),
    Stale(Option<SearchSheetIndexEntry>),
    Missing,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SearchIndexStamp {
    pub document_id: u64,
    pub generation: u64,
    pub revision: u64,
}

pub struct SearchWriterHandle {
    pub writer: Arc<Mutex<IndexWriter>>,
    pub text_field: Field,
    pub display_field: Field,
    pub row_field: Field,
    pub col_field: Field,
    pub cell_id_field: Field,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchCellText {
    pub row: usize,
    pub col: usize,
    pub search_text: String,
    pub display_text: String,
}

static NEXT_SEARCH_INDEX_GENERATION: AtomicU64 = AtomicU64::new(1);

pub struct SearchIndexStore {
    generation: u64,
    revision: u64,
    sheet_revisions: Vec<u64>,
    sheets: Vec<SearchSheetSlot>,
}

impl Default for SearchIndexStore {
    fn default() -> Self {
        Self {
            generation: NEXT_SEARCH_INDEX_GENERATION.fetch_add(1, Ordering::Relaxed),
            revision: 0,
            sheet_revisions: Vec::new(),
            sheets: Vec::new(),
        }
    }
}

impl SearchIndexStore {
    pub fn stamp(&self, document_id: u64) -> SearchIndexStamp {
        SearchIndexStamp {
            document_id,
            generation: self.generation,
            revision: self.revision,
        }
    }

    pub fn sheet_stamp(&self, document_id: u64, sheet_index: usize) -> SearchIndexStamp {
        SearchIndexStamp {
            document_id,
            generation: self.generation,
            revision: self.sheet_revision(sheet_index),
        }
    }

    pub fn mark_stale(&mut self, document_id: u64) -> SearchIndexStamp {
        self.revision = self.revision.wrapping_add(1);
        self.sheet_revisions
            .resize(self.sheets.len(), self.revision.saturating_sub(1));
        for (sheet_index, slot) in self.sheets.iter_mut().enumerate() {
            self.sheet_revisions[sheet_index] = self.sheet_revisions[sheet_index].wrapping_add(1);
            if let SearchSheetSlot::Fresh(_) = slot {
                let previous = std::mem::replace(slot, SearchSheetSlot::Missing);
                *slot = match previous {
                    SearchSheetSlot::Fresh(entry) => SearchSheetSlot::Stale(Some(entry)),
                    other => other,
                };
            }
        }
        self.stamp(document_id)
    }

    pub fn mark_sheet_stale(&mut self, sheet_index: usize) {
        self.ensure_sheet_slot(sheet_index);
        self.sheet_revisions[sheet_index] = self.sheet_revisions[sheet_index].wrapping_add(1);
        let previous = std::mem::replace(&mut self.sheets[sheet_index], SearchSheetSlot::Missing);
        self.sheets[sheet_index] = match previous {
            SearchSheetSlot::Fresh(entry) | SearchSheetSlot::Stale(Some(entry)) => {
                SearchSheetSlot::Stale(Some(entry))
            }
            SearchSheetSlot::Stale(None) | SearchSheetSlot::Missing => SearchSheetSlot::Stale(None),
        };
    }

    pub fn mark_sheet_fresh(
        &mut self,
        document_id: u64,
        sheet_index: usize,
        stamp: SearchIndexStamp,
    ) {
        if stamp != self.sheet_stamp(document_id, sheet_index) {
            return;
        }
        if let Some(slot) = self.sheets.get_mut(sheet_index)
            && matches!(slot, SearchSheetSlot::Stale(_))
        {
            let previous = std::mem::replace(slot, SearchSheetSlot::Missing);
            *slot = match previous {
                SearchSheetSlot::Stale(Some(mut entry)) if entry.revision <= stamp.revision => {
                    entry.revision = stamp.revision;
                    SearchSheetSlot::Fresh(entry)
                }
                SearchSheetSlot::Fresh(mut entry) if entry.revision <= stamp.revision => {
                    entry.revision = stamp.revision;
                    SearchSheetSlot::Fresh(entry)
                }
                SearchSheetSlot::Stale(entry) => SearchSheetSlot::Stale(entry),
                other => other,
            };
        }
    }

    pub fn install_sheet_index(
        &mut self,
        document_id: u64,
        sheet_index: usize,
        stamp: SearchIndexStamp,
        index: Option<SearchSheetIndex>,
    ) {
        if stamp != self.sheet_stamp(document_id, sheet_index) {
            return;
        }
        self.ensure_sheet_slot(sheet_index);
        self.sheets[sheet_index] = index
            .map(|index| {
                SearchSheetSlot::Fresh(SearchSheetIndexEntry {
                    revision: stamp.revision,
                    index,
                })
            })
            .unwrap_or(SearchSheetSlot::Missing);
    }

    pub fn truncate(&mut self, sheet_count: usize) {
        self.sheets.truncate(sheet_count);
        self.sheet_revisions.truncate(sheet_count);
    }

    pub fn writer_handle(
        &self,
        document_id: u64,
        sheet_index: usize,
        stamp: SearchIndexStamp,
    ) -> Option<SearchWriterHandle> {
        if stamp != self.sheet_stamp(document_id, sheet_index) {
            return None;
        }
        let entry = match self.sheets.get(sheet_index)? {
            SearchSheetSlot::Fresh(entry) => entry,
            SearchSheetSlot::Stale(Some(entry)) => entry,
            SearchSheetSlot::Stale(None) | SearchSheetSlot::Missing => return None,
        };
        if entry.revision > stamp.revision {
            return None;
        }
        let row_field = entry.index.schema.get_field("row").ok()?;
        let col_field = entry.index.schema.get_field("col").ok()?;
        Some(SearchWriterHandle {
            writer: std::sync::Arc::clone(&entry.index.writer),
            text_field: entry.index.text_field,
            display_field: entry.index.display_field,
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
    ) -> Option<Vec<SearchCellText>> {
        let query = query.trim();
        if query.is_empty() {
            return Some(vec![]);
        }

        let entry = match self.sheets.get(sheet_index)? {
            SearchSheetSlot::Fresh(entry) => entry,
            SearchSheetSlot::Stale(_) | SearchSheetSlot::Missing => return None,
        };
        if entry.revision != self.sheet_revision(sheet_index) {
            return None;
        };

        Some(search_index(&entry.index, query, limit))
    }

    fn ensure_sheet_slot(&mut self, sheet_index: usize) {
        if self.sheets.len() <= sheet_index {
            self.sheets
                .resize_with(sheet_index + 1, || SearchSheetSlot::Missing);
        }
        if self.sheet_revisions.len() <= sheet_index {
            self.sheet_revisions.resize(sheet_index + 1, self.revision);
        }
    }

    fn sheet_revision(&self, sheet_index: usize) -> u64 {
        self.sheet_revisions
            .get(sheet_index)
            .copied()
            .unwrap_or(self.revision)
    }
}

#[derive(Debug)]
pub struct SearchMatcher {
    query: String,
    query_terms: Vec<String>,
}

impl SearchMatcher {
    pub fn new(query: &str) -> Option<Self> {
        let query = query.trim();
        if query.is_empty() {
            return None;
        }
        Some(Self {
            query: query.to_lowercase(),
            query_terms: tokenize_search_text(query),
        })
    }

    pub fn matches(&self, text: &str) -> bool {
        let text = text.trim();
        if text.is_empty() {
            return false;
        }
        let text_lower = text.to_lowercase();
        if text_lower.contains(&self.query) {
            return true;
        }
        if self.query_terms.is_empty() {
            return false;
        }
        let text_terms = tokenize_search_text(text);
        self.query_terms
            .iter()
            .all(|query_term| text_terms.iter().any(|text_term| text_term == query_term))
    }
}

pub fn collect_sheet_search_text(sheet: &SheetData) -> Vec<SearchCellText> {
    sheet
        .rows
        .iter()
        .enumerate()
        .flat_map(|(row_idx, row)| {
            row.iter().enumerate().filter_map(move |(col_idx, _cell)| {
                let text = sheet.cell_search_text(row_idx, col_idx);
                let display = sheet.cell_display_text(row_idx, col_idx);
                (!text.is_empty()).then_some(SearchCellText {
                    row: row_idx,
                    col: col_idx,
                    search_text: text,
                    display_text: display,
                })
            })
        })
        .collect()
}

#[cfg(test)]
pub fn build_sheet_index(cells: &[SearchCellText]) -> Option<SearchSheetIndex> {
    build_sheet_index_with_cancel(cells, || true)
}

pub fn build_sheet_index_with_cancel(
    cells: &[SearchCellText],
    should_continue: impl Fn() -> bool,
) -> Option<SearchSheetIndex> {
    if !should_continue() {
        return None;
    }

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

    for (cell_index, cell) in cells.iter().enumerate() {
        if cell_index % 512 == 0 && !should_continue() {
            return None;
        }
        if let Err(error) = writer.add_document(doc!(
            fields.text => cell.search_text.clone(),
            fields.display => cell.display_text.clone(),
            fields.row => cell.row as u64,
            fields.col => cell.col as u64,
            fields.cell_id => format!("{}:{}", cell.row, cell.col),
        )) {
            eprintln!("Failed to add document: {error:?}");
        }
    }

    if !should_continue() {
        return None;
    }

    if let Err(error) = writer.commit() {
        eprintln!("Failed to commit index: {error:?}");
        return None;
    }

    if !should_continue() {
        return None;
    }

    Some(SearchSheetIndex {
        index,
        schema,
        text_field: fields.text,
        display_field: fields.display,
        cell_id_field: fields.cell_id,
        writer: Arc::new(Mutex::new(writer)),
    })
}

fn create_tantivy_index() -> Result<(Index, Schema, SchemaFields), tantivy::TantivyError> {
    let mut schema_builder = Schema::builder();

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
    let display_field =
        schema_builder.add_text_field("display", TextOptions::default().set_stored());
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
            display: display_field,
            row: row_field,
            col: col_field,
            cell_id: cell_id_field,
        },
    ))
}

fn search_analyzer() -> TextAnalyzer {
    TextAnalyzer::builder(JiebaTokenizer::new()).build()
}

fn tokenize_search_text(text: &str) -> Vec<String> {
    let mut analyzer = search_analyzer();
    let mut stream = analyzer.token_stream(text);
    let mut tokens = Vec::new();
    while stream.advance() {
        let token = stream.token();
        if !token.text.is_empty() {
            tokens.push(token.text.to_lowercase());
        }
    }
    tokens
}

fn search_index(index: &SearchSheetIndex, query: &str, limit: usize) -> Vec<SearchCellText> {
    let row_field = match index.schema.get_field("row") {
        Ok(field) => field,
        Err(_) => return vec![],
    };
    let col_field = match index.schema.get_field("col") {
        Ok(field) => field,
        Err(_) => return vec![],
    };
    let display_field = match index.schema.get_field("display") {
        Ok(field) => field,
        Err(_) => return vec![],
    };
    let text_field = match index.schema.get_field("text") {
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
            && let (Some(row_val), Some(col_val), Some(display_val)) = (
                doc.get_first(row_field),
                doc.get_first(col_field),
                doc.get_first(display_field),
            )
            && let (Some(row), Some(col), Some(display)) =
                (row_val.as_u64(), col_val.as_u64(), display_val.as_str())
        {
            let search_text = doc
                .get_first(text_field)
                .and_then(|value| value.as_str())
                .unwrap_or(display)
                .to_string();
            results.push(SearchCellText {
                row: row as usize,
                col: col as usize,
                search_text,
                display_text: display.to_string(),
            });
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CellFormatProjection, ReadOnlyRichProjection};
    use serde_json::Value;
    use std::collections::HashMap;

    fn index_rows(rows: &[Vec<CellValue>]) -> SearchSheetIndex {
        let sheet = SheetData {
            name: "Test".to_string(),
            rows: rows.to_vec(),
            ..Default::default()
        };
        let cells = collect_sheet_search_text(&sheet);
        build_sheet_index(&cells).expect("index")
    }

    #[test]
    fn index_build_can_be_canceled_before_work_starts() {
        let cells = vec![SearchCellText {
            row: 0,
            col: 0,
            search_text: "indexed text".to_string(),
            display_text: "indexed text".to_string(),
        }];

        assert!(build_sheet_index_with_cancel(&cells, || false).is_none());
    }

    #[test]
    fn stale_indexes_are_not_used_until_matching_replacement_installs() {
        let rows = vec![vec![CellValue::String("indexed text".to_string())]];
        let index = index_rows(&rows);
        let mut store = SearchIndexStore::default();
        let document_id = 42;
        let original_stamp = store.sheet_stamp(document_id, 0);

        store.install_sheet_index(document_id, 0, original_stamp, Some(index));
        assert_eq!(
            store.search_sheet(0, "indexed", 10),
            Some(vec![SearchCellText {
                row: 0,
                col: 0,
                search_text: "indexed text".to_string(),
                display_text: "indexed text".to_string(),
            }])
        );

        let stale_stamp = store.mark_stale(document_id);
        assert_eq!(store.search_sheet(0, "indexed", 10), None);

        let stale_index = index_rows(&rows);
        store.install_sheet_index(document_id, 0, original_stamp, Some(stale_index));
        assert_eq!(store.search_sheet(0, "indexed", 10), None);

        let replacement_index = index_rows(&rows);
        store.install_sheet_index(document_id, 0, stale_stamp, Some(replacement_index));
        assert_eq!(
            store.search_sheet(0, "indexed", 10),
            Some(vec![SearchCellText {
                row: 0,
                col: 0,
                search_text: "indexed text".to_string(),
                display_text: "indexed text".to_string(),
            }])
        );
    }

    #[test]
    fn sheet_stale_state_returns_no_index_until_marked_fresh() {
        let rows = vec![vec![CellValue::String("old indexed text".to_string())]];
        let index = index_rows(&rows);
        let mut store = SearchIndexStore::default();
        let document_id = 7;
        let stamp = store.sheet_stamp(document_id, 0);

        store.install_sheet_index(document_id, 0, stamp, Some(index));
        assert!(store.search_sheet(0, "old", 10).is_some());

        store.mark_sheet_stale(0);
        assert_eq!(store.search_sheet(0, "old", 10), None);

        store.mark_sheet_fresh(document_id, 0, stamp);
        assert_eq!(store.search_sheet(0, "old", 10), None);

        let fresh_stamp = store.sheet_stamp(document_id, 0);
        let replacement = index_rows(&rows);
        store.install_sheet_index(document_id, 0, fresh_stamp, Some(replacement));
        assert!(store.search_sheet(0, "old", 10).is_some());
    }

    #[test]
    fn fallback_matcher_supports_substring_and_token_matches() {
        let matcher = SearchMatcher::new("开发").expect("matcher");
        assert!(matcher.matches("AI应用开发工程师"));

        let matcher = SearchMatcher::new("indexed text").expect("matcher");
        assert!(matcher.matches("old indexed text value"));
        assert!(!matcher.matches("indexed only"));
    }

    #[test]
    fn collect_search_text_includes_raw_and_formatted_display() {
        let sheet = SheetData {
            name: "Test".to_string(),
            rows: vec![vec![CellValue::Number(Value::from(0.4))]],
            rich: ReadOnlyRichProjection {
                cell_formats: HashMap::from([(
                    "A1".to_string(),
                    CellFormatProjection {
                        number_format: Some("0%".to_string()),
                        style_id: None,
                    },
                )]),
                ..Default::default()
            },
            ..Default::default()
        };

        let cells = collect_sheet_search_text(&sheet);

        assert_eq!(cells[0].display_text, "40%");
        assert!(cells[0].search_text.contains("40%"));
        assert!(cells[0].search_text.contains("0.4"));
    }
}
