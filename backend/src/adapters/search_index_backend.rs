use std::sync::{Arc, Mutex};

use tantivy::collector::TopDocs;
use tantivy::directory::RamDirectory;
use tantivy::query::{BooleanQuery, Occur, Query, RegexQuery, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, STRING, Schema, TextFieldIndexing, TextOptions, Value,
};
use tantivy::tokenizer::{LowerCaser, TextAnalyzer, TokenStream};
use tantivy::{Index, IndexWriter, Order, TantivyDocument, Term, doc};
use tantivy_jieba::JiebaTokenizer;

use crate::domain::SearchCellText;
use crate::error::AppError;

fn search_text_analyzer() -> TextAnalyzer {
    TextAnalyzer::builder(JiebaTokenizer::new())
        .filter(LowerCaser)
        .build()
}

pub(crate) fn tokenize_search_text(text: &str) -> Vec<String> {
    let mut analyzer = search_text_analyzer();
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

pub(crate) const WRITER_ARENA_BYTES: usize = 15_000_000;

struct SchemaFields {
    text: Field,
    literal: Field,
    display: Field,
    row: Field,
    col: Field,
    position: Field,
    cell_id: Field,
}

pub(crate) trait SearchIndexReader: Send + Sync {
    fn search(
        &self,
        literal: &str,
        terms: &[String],
        limit: usize,
    ) -> Result<Vec<SearchCellText>, AppError>;
}

pub(crate) struct SearchIndexCellUpdate<'a> {
    pub row: usize,
    pub col: usize,
    pub search_text: &'a str,
    pub display_text: &'a str,
}

pub(crate) struct SearchSheetIndex {
    index: Index,
    directory: RamDirectory,
    fields: SchemaFields,
    writer: Arc<Mutex<IndexWriter>>,
    #[cfg(test)]
    accounted_bytes_override: Option<usize>,
    #[cfg(test)]
    query_failure_override: Option<String>,
}

pub(crate) enum SearchIndexBuildOutcome {
    Built(SearchSheetIndex),
    Cancelled,
}

pub(crate) fn build_sheet_index_with_cancel(
    cells: &[SearchCellText],
    should_continue: impl Fn() -> bool,
) -> Result<SearchIndexBuildOutcome, AppError> {
    if !should_continue() {
        return Ok(SearchIndexBuildOutcome::Cancelled);
    }

    let (index, directory, fields) =
        create_tantivy_index().map_err(|error| search_index_error("create index", error))?;
    let mut writer = index
        .writer(WRITER_ARENA_BYTES)
        .map_err(|error| search_index_error("create writer", error))?;

    for (cell_index, cell) in cells.iter().enumerate() {
        if cell_index % 512 == 0 && !should_continue() {
            return Ok(SearchIndexBuildOutcome::Cancelled);
        }
        add_cell_document(
            &mut writer,
            &fields,
            cell.row,
            cell.col,
            &cell.search_text,
            &cell.display_text,
        )?;
    }

    if !should_continue() {
        return Ok(SearchIndexBuildOutcome::Cancelled);
    }
    writer
        .commit()
        .map_err(|error| search_index_error("commit writer", error))?;
    if !should_continue() {
        return Ok(SearchIndexBuildOutcome::Cancelled);
    }

    Ok(SearchIndexBuildOutcome::Built(SearchSheetIndex {
        index,
        directory,
        fields,
        writer: Arc::new(Mutex::new(writer)),
        #[cfg(test)]
        accounted_bytes_override: None,
        #[cfg(test)]
        query_failure_override: None,
    }))
}

#[cfg(test)]
pub(crate) fn build_sheet_index(cells: &[SearchCellText]) -> Result<SearchSheetIndex, AppError> {
    match build_sheet_index_with_cancel(cells, || true)? {
        SearchIndexBuildOutcome::Built(index) => Ok(index),
        SearchIndexBuildOutcome::Cancelled => Err(AppError::Internal(
            "search index build was unexpectedly cancelled".to_string(),
        )),
    }
}

impl SearchSheetIndex {
    pub(crate) fn estimated_bytes(&self) -> usize {
        #[cfg(test)]
        if let Some(estimated_bytes) = self.accounted_bytes_override {
            return estimated_bytes;
        }
        self.directory
            .total_mem_usage()
            .saturating_add(WRITER_ARENA_BYTES)
            .saturating_add(std::mem::size_of::<Self>())
    }

    pub(crate) fn apply_updates<'a>(
        &self,
        updates: impl IntoIterator<Item = SearchIndexCellUpdate<'a>>,
    ) -> Result<(), AppError> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| AppError::poisoned_lock("search index writer"))?;
        for update in updates {
            let cell_id = format!("{}:{}", update.row, update.col);
            writer.delete_term(Term::from_field_text(self.fields.cell_id, &cell_id));
            if !update.search_text.is_empty() {
                add_cell_document(
                    &mut writer,
                    &self.fields,
                    update.row,
                    update.col,
                    update.search_text,
                    update.display_text,
                )?;
            }
        }
        writer
            .commit()
            .map(|_| ())
            .map_err(|error| search_index_error("commit incremental writer", error))
    }

    #[cfg(test)]
    pub(crate) fn fail_queries_for_test(&mut self, message: &str) {
        self.query_failure_override = Some(message.to_string());
    }

    #[cfg(test)]
    pub(crate) fn set_accounted_bytes_for_test(&mut self, bytes: usize) {
        self.accounted_bytes_override = Some(bytes);
    }

    #[cfg(test)]
    pub(crate) fn directory_memory_bytes_for_test(&self) -> usize {
        self.directory.total_mem_usage()
    }
}

impl SearchIndexReader for SearchSheetIndex {
    fn search(
        &self,
        literal: &str,
        terms: &[String],
        limit: usize,
    ) -> Result<Vec<SearchCellText>, AppError> {
        #[cfg(test)]
        if let Some(message) = &self.query_failure_override {
            return Err(AppError::Internal(message.clone()));
        }
        let reader = self
            .index
            .reader()
            .map_err(|error| search_index_error("create reader", error))?;
        let searcher = reader.searcher();
        let query = compile_index_query(self.fields.text, self.fields.literal, literal, terms)
            .ok_or_else(|| {
                AppError::Internal("failed to compile search index query".to_string())
            })?;
        let top_docs = searcher
            .search(
                &query,
                &TopDocs::with_limit(limit).order_by_fast_field::<u64>("position", Order::Asc),
            )
            .map_err(|error| search_index_error("execute query", error))?;

        let mut results = Vec::new();
        for (_score, doc_address) in top_docs {
            let doc = searcher
                .doc::<TantivyDocument>(doc_address)
                .map_err(|error| search_index_error("read query result", error))?;
            results.push(indexed_cell(&doc, &self.fields)?);
        }
        Ok(results)
    }
}

fn create_tantivy_index() -> Result<(Index, RamDirectory, SchemaFields), tantivy::TantivyError> {
    let mut schema_builder = Schema::builder();
    let text = schema_builder.add_text_field(
        "text",
        TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("jieba")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored(),
    );
    let literal = schema_builder.add_text_field("literal", STRING);
    let display = schema_builder.add_text_field("display", TextOptions::default().set_stored());
    let row = schema_builder.add_u64_field("row", tantivy::schema::FAST | tantivy::schema::STORED);
    let col = schema_builder.add_u64_field("col", tantivy::schema::FAST | tantivy::schema::STORED);
    let position = schema_builder.add_u64_field("position", tantivy::schema::FAST);
    let cell_id = schema_builder.add_text_field(
        "cell_id",
        TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("raw")
                .set_index_option(IndexRecordOption::Basic),
        ),
    );
    let schema = schema_builder.build();
    let directory = RamDirectory::create();
    let index = Index::builder()
        .schema(schema)
        .open_or_create(directory.clone())?;
    index.tokenizers().register("jieba", search_text_analyzer());
    Ok((
        index,
        directory,
        SchemaFields {
            text,
            literal,
            display,
            row,
            col,
            position,
            cell_id,
        },
    ))
}

fn add_cell_document(
    writer: &mut IndexWriter,
    fields: &SchemaFields,
    row: usize,
    col: usize,
    search_text: &str,
    display_text: &str,
) -> Result<(), AppError> {
    writer
        .add_document(doc!(
            fields.text => search_text.to_string(),
            fields.literal => search_text.to_lowercase(),
            fields.display => display_text.to_string(),
            fields.row => row as u64,
            fields.col => col as u64,
            fields.position => search_position(row, col),
            fields.cell_id => format!("{row}:{col}"),
        ))
        .map(|_| ())
        .map_err(|error| search_index_error("add document", error))
}

fn indexed_cell(
    document: &TantivyDocument,
    fields: &SchemaFields,
) -> Result<SearchCellText, AppError> {
    let row_value = document
        .get_first(fields.row)
        .ok_or_else(|| AppError::Internal("search index result has no row".to_string()))?;
    let row = row_value
        .as_u64()
        .ok_or_else(|| AppError::Internal("search index result has invalid row".to_string()))?;
    let col_value = document
        .get_first(fields.col)
        .ok_or_else(|| AppError::Internal("search index result has no column".to_string()))?;
    let col = col_value
        .as_u64()
        .ok_or_else(|| AppError::Internal("search index result has invalid column".to_string()))?;
    let display_value = document
        .get_first(fields.display)
        .ok_or_else(|| AppError::Internal("search index result has no display text".to_string()))?;
    let display = display_value.as_str().ok_or_else(|| {
        AppError::Internal("search index result has invalid display text".to_string())
    })?;
    let search_text = document
        .get_first(fields.text)
        .map(|value| {
            value.as_str().ok_or_else(|| {
                AppError::Internal("search index result has invalid search text".to_string())
            })
        })
        .transpose()?
        .unwrap_or(display);
    Ok(SearchCellText {
        row: usize::try_from(row)
            .map_err(|_| AppError::Internal("search index row is out of range".to_string()))?,
        col: usize::try_from(col)
            .map_err(|_| AppError::Internal("search index column is out of range".to_string()))?,
        search_text: search_text.to_string(),
        display_text: display.to_string(),
    })
}

fn compile_index_query(
    text_field: Field,
    literal_field: Field,
    literal: &str,
    terms: &[String],
) -> Option<BooleanQuery> {
    let term_clauses: Vec<(Occur, Box<dyn Query>)> = terms
        .iter()
        .map(|term| {
            let query = TermQuery::new(
                Term::from_field_text(text_field, term),
                IndexRecordOption::Basic,
            );
            (Occur::Must, Box::new(query) as Box<dyn Query>)
        })
        .collect();
    let mut alternatives = Vec::<(Occur, Box<dyn Query>)>::new();
    let literal_pattern = format!(".*{}.*", escape_regex_literal(literal));
    if let Ok(query) = RegexQuery::from_pattern(&literal_pattern, literal_field) {
        alternatives.push((Occur::Should, Box::new(query)));
    }
    if !term_clauses.is_empty() {
        alternatives.push((Occur::Should, Box::new(BooleanQuery::new(term_clauses))));
    }
    (!alternatives.is_empty()).then(|| BooleanQuery::new(alternatives))
}

fn escape_regex_literal(literal: &str) -> String {
    let mut escaped = String::with_capacity(literal.len());
    for character in literal.chars() {
        if matches!(
            character,
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
        ) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn search_position(row: usize, col: usize) -> u64 {
    ((row as u64) << 32) | (col as u64 & u32::MAX as u64)
}

fn search_index_error(operation: &str, error: impl std::fmt::Debug) -> AppError {
    AppError::Internal(format!("failed to {operation} for search index: {error:?}"))
}
