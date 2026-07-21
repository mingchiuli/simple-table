use std::collections::{HashMap, HashSet, VecDeque};
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
use crate::editor_protocol::MAX_SEARCH_QUERY_BYTES;
use crate::error::AppError;
#[cfg(test)]
use crate::types::CellValue;

pub(crate) const WRITER_ARENA_BYTES: usize = 15_000_000;
pub(crate) const MAX_RESIDENT_SEARCH_INDEXES: usize = 4;
pub(crate) const MAX_RESIDENT_SEARCH_INDEX_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_SEARCH_QUERY_TERMS: usize = 64;

struct SchemaFields {
    text: Field,
    literal: Field,
    display: Field,
    row: Field,
    col: Field,
    position: Field,
    cell_id: Field,
}

pub struct SearchSheetIndex {
    index: Index,
    directory: RamDirectory,
    schema: Schema,
    text_field: Field,
    literal_field: Field,
    display_field: Field,
    cell_id_field: Field,
    writer: Arc<Mutex<IndexWriter>>,
    #[cfg(test)]
    accounted_bytes_override: Option<usize>,
}

#[derive(Default)]
pub(crate) struct RetiredSearchIndexes {
    indexes: Vec<Arc<SearchSheetIndex>>,
}

impl RetiredSearchIndexes {
    pub(crate) fn append(&mut self, mut other: Self) {
        self.indexes.append(&mut other.indexes);
    }

    fn push(&mut self, index: Arc<SearchSheetIndex>) {
        self.indexes.push(index);
    }

    fn push_slot(&mut self, slot: SearchSheetSlot) {
        match slot {
            SearchSheetSlot::Fresh(entry)
            | SearchSheetSlot::Stale {
                entry: Some(entry), ..
            } => self.push(entry.index),
            SearchSheetSlot::Stale { entry: None, .. } | SearchSheetSlot::Missing => {}
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.indexes.len()
    }
}

struct SearchSheetIndexEntry {
    revision: u64,
    index: Arc<SearchSheetIndex>,
}

enum SearchSheetSlot {
    Fresh(SearchSheetIndexEntry),
    Stale {
        entry: Option<SearchSheetIndexEntry>,
        incremental_allowed: bool,
    },
    Missing,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SearchIndexStamp {
    pub document_id: u64,
    pub generation: u64,
    pub source_revision: u64,
    pub revision: u64,
}

pub struct SearchWriterHandle {
    pub writer: Arc<Mutex<IndexWriter>>,
    pub text_field: Field,
    pub literal_field: Field,
    pub display_field: Field,
    pub row_field: Field,
    pub col_field: Field,
    pub position_field: Field,
    pub cell_id_field: Field,
}

pub struct SearchIndexStore {
    generation: u64,
    source_revision: u64,
    revision: u64,
    sheet_revisions: Vec<u64>,
    sheets: Vec<SearchSheetSlot>,
    resident_order: VecDeque<usize>,
}

#[derive(Default)]
pub(crate) struct SearchIndexRegistry {
    documents: HashMap<u64, SearchIndexStore>,
}

impl SearchIndexRegistry {
    pub(crate) fn document(&self, document_id: u64) -> Option<&SearchIndexStore> {
        self.documents.get(&document_id)
    }

    pub(crate) fn document_mut(&mut self, document_id: u64) -> &mut SearchIndexStore {
        self.documents.entry(document_id).or_default()
    }

    pub(crate) fn synchronize_revision(
        &mut self,
        document_id: u64,
        source_revision: u64,
    ) -> Option<&mut SearchIndexStore> {
        let store = self.document_mut(document_id);
        if store.source_revision() > source_revision {
            return None;
        }
        if store.source_revision() != source_revision {
            store.set_source_revision(source_revision);
            store.mark_stale(document_id);
        }
        Some(store)
    }

    pub(crate) fn remove(&mut self, document_id: u64) -> Option<SearchIndexStore> {
        self.documents.remove(&document_id)
    }
}

impl Default for SearchIndexStore {
    fn default() -> Self {
        Self {
            generation: nonzero_random_u64(),
            source_revision: 0,
            revision: 0,
            sheet_revisions: Vec::new(),
            sheets: Vec::new(),
            resident_order: VecDeque::new(),
        }
    }
}

impl SearchIndexStore {
    pub fn stamp(&self, document_id: u64) -> SearchIndexStamp {
        SearchIndexStamp {
            document_id,
            generation: self.generation,
            source_revision: self.source_revision,
            revision: self.revision,
        }
    }

    pub fn sheet_stamp(&self, document_id: u64, sheet_index: usize) -> SearchIndexStamp {
        SearchIndexStamp {
            document_id,
            generation: self.generation,
            source_revision: self.source_revision,
            revision: self.sheet_revision(sheet_index),
        }
    }

    pub fn source_revision(&self) -> u64 {
        self.source_revision
    }

    pub fn set_source_revision(&mut self, source_revision: u64) {
        self.source_revision = source_revision;
    }

    pub fn mark_stale(&mut self, document_id: u64) -> SearchIndexStamp {
        if let Some(revision) = self.revision.checked_add(1) {
            self.revision = revision;
        } else {
            self.rotate_generation();
        }
        self.sheet_revisions
            .resize(self.sheets.len(), self.revision.saturating_sub(1));
        for (sheet_index, slot) in self.sheets.iter_mut().enumerate() {
            self.sheet_revisions[sheet_index] = self.sheet_revisions[sheet_index]
                .checked_add(1)
                .unwrap_or(0);
            let previous = std::mem::replace(slot, SearchSheetSlot::Missing);
            *slot = match previous {
                SearchSheetSlot::Fresh(entry)
                | SearchSheetSlot::Stale {
                    entry: Some(entry), ..
                } => SearchSheetSlot::Stale {
                    entry: Some(entry),
                    incremental_allowed: false,
                },
                SearchSheetSlot::Stale { entry: None, .. } | SearchSheetSlot::Missing => {
                    SearchSheetSlot::Stale {
                        entry: None,
                        incremental_allowed: false,
                    }
                }
            };
        }
        self.stamp(document_id)
    }

    pub fn mark_sheet_stale(&mut self, sheet_index: usize) {
        self.ensure_sheet_slot(sheet_index);
        let Some(next_revision) = self.sheet_revisions[sheet_index].checked_add(1) else {
            self.rotate_generation();
            self.ensure_sheet_slot(sheet_index);
            return self.mark_sheet_stale(sheet_index);
        };
        self.sheet_revisions[sheet_index] = next_revision;
        let previous = std::mem::replace(&mut self.sheets[sheet_index], SearchSheetSlot::Missing);
        self.sheets[sheet_index] = match previous {
            SearchSheetSlot::Fresh(entry) => SearchSheetSlot::Stale {
                entry: Some(entry),
                incremental_allowed: true,
            },
            SearchSheetSlot::Stale {
                entry,
                incremental_allowed,
            } => SearchSheetSlot::Stale {
                entry,
                incremental_allowed,
            },
            SearchSheetSlot::Missing => SearchSheetSlot::Stale {
                entry: None,
                incremental_allowed: true,
            },
        };
    }

    pub fn mark_sheet_fresh(
        &mut self,
        document_id: u64,
        sheet_index: usize,
        stamp: SearchIndexStamp,
    ) -> RetiredSearchIndexes {
        if stamp != self.sheet_stamp(document_id, sheet_index) {
            return RetiredSearchIndexes::default();
        }
        if let Some(slot) = self.sheets.get_mut(sheet_index)
            && matches!(slot, SearchSheetSlot::Stale { .. })
        {
            let previous = std::mem::replace(slot, SearchSheetSlot::Missing);
            *slot = match previous {
                SearchSheetSlot::Stale {
                    entry: Some(mut entry),
                    incremental_allowed: true,
                } if entry.revision <= stamp.revision => {
                    entry.revision = stamp.revision;
                    SearchSheetSlot::Fresh(entry)
                }
                SearchSheetSlot::Fresh(mut entry) if entry.revision <= stamp.revision => {
                    entry.revision = stamp.revision;
                    SearchSheetSlot::Fresh(entry)
                }
                SearchSheetSlot::Stale {
                    entry,
                    incremental_allowed,
                } => SearchSheetSlot::Stale {
                    entry,
                    incremental_allowed,
                },
                other => other,
            };
        }
        self.enforce_resident_byte_budget()
    }

    pub fn install_sheet_index(
        &mut self,
        document_id: u64,
        sheet_index: usize,
        stamp: SearchIndexStamp,
        index: Option<SearchSheetIndex>,
    ) -> RetiredSearchIndexes {
        let mut retired = RetiredSearchIndexes::default();
        let mut incoming = index.map(Arc::new);
        if stamp != self.sheet_stamp(document_id, sheet_index) {
            if let Some(index) = incoming.take() {
                retired.push(index);
            }
            return retired;
        }
        self.ensure_sheet_slot(sheet_index);
        if let Some(previous) = self.remove_resident_index(sheet_index) {
            retired.push(previous);
        }
        self.sheets[sheet_index] = match incoming.take() {
            Some(index) => {
                let estimated_bytes = index.estimated_bytes();
                if estimated_bytes > MAX_RESIDENT_SEARCH_INDEX_BYTES {
                    retired.push(index);
                    return retired;
                }
                self.evict_resident_until_bounded(estimated_bytes, &mut retired);
                if self.resident_index_bytes().saturating_add(estimated_bytes)
                    > MAX_RESIDENT_SEARCH_INDEX_BYTES
                {
                    retired.push(index);
                    return retired;
                }
                self.resident_order.push_back(sheet_index);
                SearchSheetSlot::Fresh(SearchSheetIndexEntry {
                    revision: stamp.revision,
                    index,
                })
            }
            None => SearchSheetSlot::Missing,
        };
        retired
    }

    pub fn truncate(&mut self, sheet_count: usize) -> RetiredSearchIndexes {
        let mut retired = RetiredSearchIndexes::default();
        if sheet_count < self.sheets.len() {
            for slot in self.sheets.drain(sheet_count..) {
                retired.push_slot(slot);
            }
        }
        self.sheet_revisions.truncate(sheet_count);
        self.resident_order
            .retain(|sheet_index| *sheet_index < sheet_count);
        retired
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
            SearchSheetSlot::Stale {
                entry: Some(entry),
                incremental_allowed: true,
            } => entry,
            SearchSheetSlot::Stale { .. } | SearchSheetSlot::Missing => return None,
        };
        if entry.revision > stamp.revision {
            return None;
        }
        let row_field = entry.index.schema.get_field("row").ok()?;
        let col_field = entry.index.schema.get_field("col").ok()?;
        let position_field = entry.index.schema.get_field("position").ok()?;
        Some(SearchWriterHandle {
            writer: Arc::clone(&entry.index.writer),
            text_field: entry.index.text_field,
            literal_field: entry.index.literal_field,
            display_field: entry.index.display_field,
            row_field,
            col_field,
            position_field,
            cell_id_field: entry.index.cell_id_field,
        })
    }

    #[cfg(test)]
    pub fn search_sheet(
        &self,
        sheet_index: usize,
        query: &str,
        limit: usize,
    ) -> Option<Vec<SearchCellText>> {
        let plan = SearchQueryPlan::new(query)?;
        self.fresh_sheet_index(sheet_index)
            .map(|index| index.search(&plan, limit))
    }

    pub fn fresh_sheet_index(&self, sheet_index: usize) -> Option<Arc<SearchSheetIndex>> {
        let entry = match self.sheets.get(sheet_index)? {
            SearchSheetSlot::Fresh(entry) => entry,
            SearchSheetSlot::Stale { .. } | SearchSheetSlot::Missing => return None,
        };
        (entry.revision == self.sheet_revision(sheet_index)).then(|| Arc::clone(&entry.index))
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

    fn rotate_generation(&mut self) {
        self.generation = nonzero_random_u64();
        self.revision = 0;
        self.sheet_revisions.fill(0);
        for slot in &mut self.sheets {
            let previous = std::mem::replace(slot, SearchSheetSlot::Missing);
            *slot = match previous {
                SearchSheetSlot::Fresh(entry)
                | SearchSheetSlot::Stale {
                    entry: Some(entry), ..
                } => SearchSheetSlot::Stale {
                    entry: Some(entry),
                    incremental_allowed: false,
                },
                SearchSheetSlot::Stale { entry: None, .. } | SearchSheetSlot::Missing => {
                    SearchSheetSlot::Stale {
                        entry: None,
                        incremental_allowed: false,
                    }
                }
            };
        }
    }

    fn remove_resident_index(&mut self, sheet_index: usize) -> Option<Arc<SearchSheetIndex>> {
        self.resident_order
            .retain(|resident_sheet| *resident_sheet != sheet_index);
        let slot = self.sheets.get_mut(sheet_index)?;
        let previous = std::mem::replace(slot, SearchSheetSlot::Missing);
        match previous {
            SearchSheetSlot::Fresh(entry)
            | SearchSheetSlot::Stale {
                entry: Some(entry), ..
            } => Some(entry.index),
            SearchSheetSlot::Stale { entry: None, .. } | SearchSheetSlot::Missing => None,
        }
    }

    fn evict_resident_until_bounded(
        &mut self,
        incoming_bytes: usize,
        retired: &mut RetiredSearchIndexes,
    ) {
        while self.resident_order.len() >= MAX_RESIDENT_SEARCH_INDEXES
            || self.resident_index_bytes().saturating_add(incoming_bytes)
                > MAX_RESIDENT_SEARCH_INDEX_BYTES
        {
            let Some(sheet_index) = self.resident_order.pop_front() else {
                return;
            };
            if let Some(index) = self.remove_resident_index(sheet_index) {
                retired.push(index);
            }
        }
    }

    #[cfg(test)]
    fn resident_index_count(&self) -> usize {
        self.resident_order.len()
    }

    fn resident_index_bytes(&self) -> usize {
        self.sheets.iter().map(search_sheet_slot_bytes).sum()
    }

    fn enforce_resident_byte_budget(&mut self) -> RetiredSearchIndexes {
        let mut retired = RetiredSearchIndexes::default();
        while self.resident_index_bytes() > MAX_RESIDENT_SEARCH_INDEX_BYTES {
            let Some(sheet_index) = self.resident_order.pop_front() else {
                return retired;
            };
            if let Some(index) = self.remove_resident_index(sheet_index) {
                retired.push(index);
            }
        }
        retired
    }
}

fn search_sheet_slot_bytes(slot: &SearchSheetSlot) -> usize {
    match slot {
        SearchSheetSlot::Fresh(entry)
        | SearchSheetSlot::Stale {
            entry: Some(entry), ..
        } => entry.index.estimated_bytes(),
        SearchSheetSlot::Stale { entry: None, .. } | SearchSheetSlot::Missing => 0,
    }
}

fn nonzero_random_u64() -> u64 {
    loop {
        let value = uuid::Uuid::new_v4().as_u128() as u64;
        if value != 0 {
            return value;
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchQueryPlan {
    literal: String,
    terms: Vec<String>,
}

impl SearchQueryPlan {
    #[cfg(test)]
    pub fn new(query: &str) -> Option<Self> {
        Self::try_new(query).ok().flatten()
    }

    pub fn try_new(query: &str) -> Result<Option<Self>, AppError> {
        if query.len() > MAX_SEARCH_QUERY_BYTES {
            return Err(AppError::ResourceLimitExceeded(format!(
                "search query requires {} bytes; the maximum is {MAX_SEARCH_QUERY_BYTES} bytes",
                query.len()
            )));
        }
        let query = query.trim();
        if query.is_empty() {
            return Ok(None);
        }
        let mut terms = Vec::new();
        let mut unique_terms = HashSet::new();
        for term in tokenize_search_text(query) {
            if !unique_terms.insert(term.clone()) {
                continue;
            }
            if terms.len() >= MAX_SEARCH_QUERY_TERMS {
                return Err(AppError::ResourceLimitExceeded(format!(
                    "search query contains more than {MAX_SEARCH_QUERY_TERMS} terms"
                )));
            }
            terms.push(term);
        }
        Ok(Some(Self {
            literal: query.to_lowercase(),
            terms,
        }))
    }

    pub fn matches(&self, text: &str) -> bool {
        let text = text.trim();
        if text.is_empty() {
            return false;
        }
        let text_lower = text.to_lowercase();
        if text_lower.contains(&self.literal) {
            return true;
        }
        if self.terms.is_empty() {
            return false;
        }
        let text_terms: HashSet<_> = tokenize_search_text(text).into_iter().collect();
        self.terms
            .iter()
            .all(|query_term| text_terms.contains(query_term))
    }

    pub(crate) fn first_match_byte(&self, text: &str) -> Option<usize> {
        let lowercase = text.to_lowercase();
        std::iter::once(&self.literal)
            .chain(self.terms.iter())
            .filter_map(|term| lowercase.find(term))
            .filter(|index| *index <= text.len() && text.is_char_boundary(*index))
            .min()
    }

    fn terms(&self) -> &[String] {
        &self.terms
    }
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

    let (index, directory, schema, fields) = match create_tantivy_index() {
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
            fields.literal => cell.search_text.to_lowercase(),
            fields.display => cell.display_text.clone(),
            fields.row => cell.row as u64,
            fields.col => cell.col as u64,
            fields.position => search_position(cell.row, cell.col),
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
        directory,
        schema,
        text_field: fields.text,
        literal_field: fields.literal,
        display_field: fields.display,
        cell_id_field: fields.cell_id,
        writer: Arc::new(Mutex::new(writer)),
        #[cfg(test)]
        accounted_bytes_override: None,
    })
}

fn create_tantivy_index()
-> Result<(Index, RamDirectory, Schema, SchemaFields), tantivy::TantivyError> {
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
    let literal_field = schema_builder.add_text_field("literal", STRING);
    let display_field =
        schema_builder.add_text_field("display", TextOptions::default().set_stored());
    let row_field =
        schema_builder.add_u64_field("row", tantivy::schema::FAST | tantivy::schema::STORED);
    let col_field =
        schema_builder.add_u64_field("col", tantivy::schema::FAST | tantivy::schema::STORED);
    let position_field = schema_builder.add_u64_field("position", tantivy::schema::FAST);
    let cell_id_field = schema_builder.add_text_field(
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
        .schema(schema.clone())
        .open_or_create(directory.clone())?;
    let analyzer = TextAnalyzer::builder(JiebaTokenizer::new())
        .filter(LowerCaser)
        .build();
    index.tokenizers().register("jieba", analyzer);

    Ok((
        index,
        directory,
        schema,
        SchemaFields {
            text: text_field,
            literal: literal_field,
            display: display_field,
            row: row_field,
            col: col_field,
            position: position_field,
            cell_id: cell_id_field,
        },
    ))
}

fn search_analyzer() -> TextAnalyzer {
    TextAnalyzer::builder(JiebaTokenizer::new())
        .filter(LowerCaser)
        .build()
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

fn search_index(
    index: &SearchSheetIndex,
    plan: &SearchQueryPlan,
    limit: usize,
) -> Vec<SearchCellText> {
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
    let Some(query) = compile_index_query(index.text_field, index.literal_field, plan) else {
        return vec![];
    };
    let top_docs = match searcher.search(
        &query,
        &TopDocs::with_limit(limit).order_by_fast_field::<u64>("position", Order::Asc),
    ) {
        Ok(docs) => docs,
        Err(error) => {
            eprintln!("Search failed: {error:?}");
            return vec![];
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
            if !plan.matches(&search_text) {
                continue;
            }
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

pub(crate) fn search_position(row: usize, col: usize) -> u64 {
    ((row as u64) << 32) | (col as u64 & u32::MAX as u64)
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

    pub fn search(&self, plan: &SearchQueryPlan, limit: usize) -> Vec<SearchCellText> {
        search_index(self, plan, limit)
    }
}

fn compile_index_query(
    text_field: Field,
    literal_field: Field,
    plan: &SearchQueryPlan,
) -> Option<BooleanQuery> {
    let term_clauses: Vec<(Occur, Box<dyn Query>)> = plan
        .terms()
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
    let literal_pattern = format!(".*{}.*", escape_regex_literal(&plan.literal));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::search_document_source_adapter::collect_sheet_search_text;
    use crate::document_data::DocumentSheet;
    use crate::document_data::{CellFormat, RichMetadata};
    use crate::domain::CellNumber;
    use crate::domain::SearchScanCursor;
    use crate::state::search_document::collect_sheet_search_text_chunk;
    use std::collections::HashMap;

    fn index_rows(rows: &[Vec<CellValue>]) -> SearchSheetIndex {
        let sheet = DocumentSheet {
            name: "Test".to_string(),
            rows: rows.to_vec(),
            ..Default::default()
        };
        let cells = collect_sheet_search_text(&sheet);
        build_sheet_index(&cells).expect("index")
    }

    #[test]
    fn registry_revision_never_moves_backwards_for_a_stale_reader() {
        let mut registry = SearchIndexRegistry::default();
        assert!(registry.synchronize_revision(7, 5).is_some());

        assert!(registry.synchronize_revision(7, 4).is_none());
        assert_eq!(registry.document(7).unwrap().source_revision(), 5);
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
    fn cell_stale_index_can_be_incrementally_updated() {
        let rows = vec![vec![CellValue::String("old indexed text".to_string())]];
        let index = index_rows(&rows);
        let mut store = SearchIndexStore::default();
        let document_id = 7;
        let stamp = store.sheet_stamp(document_id, 0);

        store.install_sheet_index(document_id, 0, stamp, Some(index));
        store.mark_sheet_stale(0);
        let stale_stamp = store.sheet_stamp(document_id, 0);

        assert!(store.writer_handle(document_id, 0, stale_stamp).is_some());
        store.mark_sheet_fresh(document_id, 0, stale_stamp);
        assert!(store.search_sheet(0, "old", 10).is_some());
    }

    #[test]
    fn rebuild_required_stale_index_cannot_be_incrementally_updated() {
        let rows = vec![vec![CellValue::String("old indexed text".to_string())]];
        let index = index_rows(&rows);
        let mut store = SearchIndexStore::default();
        let document_id = 7;
        let stamp = store.sheet_stamp(document_id, 0);

        store.install_sheet_index(document_id, 0, stamp, Some(index));
        store.mark_stale(document_id);
        let rebuild_stamp = store.sheet_stamp(document_id, 0);

        assert!(store.writer_handle(document_id, 0, rebuild_stamp).is_none());

        store.mark_sheet_stale(0);
        let later_stamp = store.sheet_stamp(document_id, 0);

        assert!(store.writer_handle(document_id, 0, later_stamp).is_none());
        store.mark_sheet_fresh(document_id, 0, later_stamp);
        assert_eq!(store.search_sheet(0, "old", 10), None);
    }

    #[test]
    fn query_plan_supports_literal_and_all_token_matches() {
        let matcher = SearchQueryPlan::new("开发").expect("matcher");
        assert!(matcher.matches("AI应用开发工程师"));

        let matcher = SearchQueryPlan::new("indexed text").expect("matcher");
        assert!(matcher.matches("old indexed text value"));
        assert!(!matcher.matches("indexed only"));
    }

    #[test]
    fn indexed_and_scan_plans_use_the_same_multi_term_semantics() {
        let rows = vec![
            vec![CellValue::String("alpha only".to_string())],
            vec![CellValue::String("beta only".to_string())],
            vec![CellValue::String("alpha and beta".to_string())],
        ];
        let sheet = DocumentSheet {
            name: "Test".to_string(),
            rows: rows.clone(),
            ..Default::default()
        };
        let cells = collect_sheet_search_text(&sheet);
        let plan = SearchQueryPlan::new("alpha beta").expect("query plan");
        let index = index_rows(&rows);
        let indexed: Vec<_> = search_index(&index, &plan, 10)
            .into_iter()
            .map(|cell| (cell.row, cell.col))
            .collect();
        let scanned: Vec<_> = cells
            .iter()
            .filter(|cell| plan.matches(&cell.search_text))
            .map(|cell| (cell.row, cell.col))
            .collect();

        assert_eq!(indexed, scanned);
        assert_eq!(indexed, vec![(2, 0)]);
    }

    #[test]
    fn indexed_and_scan_plans_preserve_literal_substring_semantics() {
        let rows = vec![
            vec![CellValue::String("alpha".to_string())],
            vec![CellValue::String("other".to_string())],
            vec![CellValue::String("cost (net)".to_string())],
        ];
        let sheet = DocumentSheet {
            name: "Test".to_string(),
            rows: rows.clone(),
            ..Default::default()
        };
        let cells = collect_sheet_search_text(&sheet);
        let index = index_rows(&rows);

        for query in ["pha", "(net)"] {
            let plan = SearchQueryPlan::new(query).expect("query plan");
            let mut indexed: Vec<_> = search_index(&index, &plan, 10)
                .into_iter()
                .map(|cell| (cell.row, cell.col))
                .collect();
            let scanned: Vec<_> = cells
                .iter()
                .filter(|cell| plan.matches(&cell.search_text))
                .map(|cell| (cell.row, cell.col))
                .collect();
            indexed.sort_unstable();

            assert_eq!(indexed, scanned, "query {query}");
        }
    }

    #[test]
    fn indexed_and_scan_plans_apply_limits_in_sheet_order() {
        let rows = vec![
            vec![
                CellValue::String("match a".to_string()),
                CellValue::String("match b".to_string()),
            ],
            vec![CellValue::String("match c".to_string())],
        ];
        let sheet = DocumentSheet {
            name: "Test".to_string(),
            rows: rows.clone(),
            ..Default::default()
        };
        let cells = collect_sheet_search_text(&sheet);
        let plan = SearchQueryPlan::new("match").expect("query plan");
        let index = index_rows(&rows);
        let indexed: Vec<_> = search_index(&index, &plan, 2)
            .into_iter()
            .map(|cell| (cell.row, cell.col))
            .collect();
        let scanned: Vec<_> = cells
            .iter()
            .filter(|cell| plan.matches(&cell.search_text))
            .take(2)
            .map(|cell| (cell.row, cell.col))
            .collect();

        assert_eq!(indexed, scanned);
        assert_eq!(indexed, vec![(0, 0), (0, 1)]);
    }

    #[test]
    fn collect_search_text_includes_raw_and_formatted_display() {
        let sheet = DocumentSheet {
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
        };

        let cells = collect_sheet_search_text(&sheet);

        assert_eq!(cells[0].display_text, "40%");
        assert!(cells[0].search_text.contains("40%"));
        assert!(cells[0].search_text.contains("0.4"));
    }

    #[test]
    fn chunked_search_text_scan_obeys_text_and_cell_budgets() {
        let sheet = DocumentSheet {
            name: "Sheet1".to_string(),
            rows: vec![
                vec![CellValue::String("one".to_string()), CellValue::Null],
                vec![
                    CellValue::String("two".to_string()),
                    CellValue::String("three".to_string()),
                ],
            ],
            ..Default::default()
        };
        let first =
            collect_sheet_search_text_chunk(&sheet, SearchScanCursor::default(), usize::MAX, 2);

        assert_eq!(first.cells.len(), 1);
        assert_eq!(first.next, Some(SearchScanCursor { row: 0, col: 2 }));

        let second = collect_sheet_search_text_chunk(
            &sheet,
            first.next.expect("next cursor"),
            7,
            usize::MAX,
        );
        assert_eq!(
            second
                .cells
                .iter()
                .map(|cell| cell.search_text.as_str())
                .collect::<Vec<_>>(),
            vec!["two"]
        );
        assert_eq!(second.next, Some(SearchScanCursor { row: 1, col: 1 }));

        let final_chunk = collect_sheet_search_text_chunk(
            &sheet,
            second.next.expect("final cursor"),
            1,
            usize::MAX,
        );
        assert_eq!(final_chunk.cells[0].search_text, "three");
        assert!(final_chunk.next.is_none());
    }

    #[test]
    fn resident_indexes_are_evicted_at_the_memory_limit() {
        let document_id = 7;
        let mut store = SearchIndexStore::default();
        let mut retired_count = 0;

        for sheet_index in 0..=MAX_RESIDENT_SEARCH_INDEXES {
            let stamp = store.sheet_stamp(document_id, sheet_index);
            retired_count += store
                .install_sheet_index(document_id, sheet_index, stamp, build_sheet_index(&[]))
                .len();
        }

        assert_eq!(retired_count, 1);
        assert_eq!(store.resident_index_count(), MAX_RESIDENT_SEARCH_INDEXES);
        assert!(store.fresh_sheet_index(0).is_none());
        assert!(
            store
                .fresh_sheet_index(MAX_RESIDENT_SEARCH_INDEXES)
                .is_some()
        );
    }

    #[test]
    fn resident_indexes_are_evicted_by_measured_bytes() {
        let document_id = 7;
        let mut store = SearchIndexStore::default();

        for sheet_index in 0..3 {
            let stamp = store.sheet_stamp(document_id, sheet_index);
            let mut index = build_sheet_index(&[]).expect("index");
            index.accounted_bytes_override = Some(24 * 1024 * 1024);
            store.install_sheet_index(document_id, sheet_index, stamp, Some(index));
        }

        assert_eq!(store.resident_index_count(), 2);
        assert!(store.resident_index_bytes() <= MAX_RESIDENT_SEARCH_INDEX_BYTES);
        assert!(store.fresh_sheet_index(0).is_none());
        assert!(store.fresh_sheet_index(2).is_some());
    }

    #[test]
    fn rejected_oversized_index_is_returned_for_lock_external_drop() {
        let document_id = 7;
        let mut store = SearchIndexStore::default();
        let stamp = store.sheet_stamp(document_id, 0);
        let mut index = build_sheet_index(&[]).expect("index");
        index.accounted_bytes_override = Some(MAX_RESIDENT_SEARCH_INDEX_BYTES + 1);

        let retired = store.install_sheet_index(document_id, 0, stamp, Some(index));

        assert_eq!(retired.len(), 1);
        assert!(store.fresh_sheet_index(0).is_none());
    }

    #[test]
    fn measured_index_bytes_include_directory_and_writer_memory() {
        let index = build_sheet_index(&[SearchCellText {
            row: 0,
            col: 0,
            search_text: "searchable".to_string(),
            display_text: "searchable".to_string(),
        }])
        .expect("index");

        assert!(index.directory.total_mem_usage() > 0);
        assert!(index.estimated_bytes() > WRITER_ARENA_BYTES);
    }
}
