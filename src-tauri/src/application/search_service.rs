use std::sync::Arc;

use crate::application::search_ports::{SearchDocumentSourcePort, SearchIndexPort};
use crate::domain::{
    SearchCellText, SearchDocumentSnapshot, SearchIndexWork, SearchScanCursor, SearchSheetSnapshot,
    SearchTextChunk,
};
use crate::error::AppError;
use crate::state::state::ActiveDocumentRepository;
use crate::types::{SearchResponse, SearchScope};

pub(crate) fn collect_sheet_search_text(sheet: &crate::types::SheetData) -> Vec<SearchCellText> {
    sheet
        .rows
        .iter()
        .enumerate()
        .flat_map(|(row_idx, row)| {
            row.iter().enumerate().filter_map(move |(col_idx, _cell)| {
                let search_text = sheet.cell_search_text(row_idx, col_idx);
                let display_text = sheet.cell_display_text(row_idx, col_idx);
                (!search_text.is_empty()).then_some(SearchCellText {
                    row: row_idx,
                    col: col_idx,
                    search_text,
                    display_text,
                })
            })
        })
        .collect()
}

#[derive(Clone)]
pub(crate) struct RepositorySearchDocumentSource {
    documents: ActiveDocumentRepository,
}

impl RepositorySearchDocumentSource {
    pub(crate) fn new(documents: ActiveDocumentRepository) -> Self {
        Self { documents }
    }

    fn read_document(
        &self,
        document_id: u64,
        expected_revision: Option<u64>,
    ) -> Result<Arc<crate::state::state::DocumentHandle>, AppError> {
        let handle = self.documents.read_handle(document_id)?;
        if let Some(revision) = expected_revision {
            drop(handle.read_for_command(document_id, revision)?);
        }
        Ok(handle)
    }
}

impl SearchDocumentSourcePort for RepositorySearchDocumentSource {
    fn document_snapshot(
        &self,
        document_id: u64,
        expected_revision: Option<u64>,
    ) -> Result<Option<SearchDocumentSnapshot>, AppError> {
        let handle = self.read_document(document_id, expected_revision)?;
        let editor = match expected_revision {
            Some(revision) => handle.read_for_command(document_id, revision)?,
            None => handle.read()?,
        };
        Ok(Some(SearchDocumentSnapshot {
            revision: editor.revision(),
            sheets: editor
                .file_data()
                .sheets
                .iter()
                .enumerate()
                .map(|(sheet_index, sheet)| SearchSheetSnapshot {
                    name: sheet.name.clone(),
                    estimated_source_bytes: editor
                        .search_sheet_snapshot_estimated_bytes(sheet_index)
                        .unwrap_or_default(),
                })
                .collect(),
        }))
    }

    fn sheet_text_snapshot(
        &self,
        document_id: u64,
        expected_revision: u64,
        sheet_index: usize,
    ) -> Result<Option<Arc<[SearchCellText]>>, AppError> {
        let handle = self.read_document(document_id, Some(expected_revision))?;
        let sheet = handle
            .read_for_command(document_id, expected_revision)?
            .file_data()
            .sheets
            .get(sheet_index)
            .map(crate::types::SheetData::search_snapshot);
        Ok(sheet.map(|sheet| Arc::from(collect_sheet_search_text(&sheet))))
    }

    fn sheet_text_chunk(
        &self,
        document_id: u64,
        expected_revision: u64,
        sheet_index: usize,
        cursor: SearchScanCursor,
        maximum_text_bytes: usize,
        maximum_cells: usize,
    ) -> Result<Option<SearchTextChunk>, AppError> {
        let handle = self.read_document(document_id, Some(expected_revision))?;
        let editor = handle.read_for_command(document_id, expected_revision)?;
        Ok(editor.search_sheet_text_chunk(sheet_index, cursor, maximum_text_bytes, maximum_cells))
    }
}

#[derive(Clone)]
pub struct SearchService {
    indexes: Arc<dyn SearchIndexPort>,
}

impl SearchService {
    pub(crate) fn from_port(indexes: Arc<dyn SearchIndexPort>) -> Self {
        Self { indexes }
    }

    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::from_port(Arc::new(NoopSearchIndexPort))
    }

    pub fn search(
        &self,
        document_id: u64,
        base_revision: u64,
        query: &str,
        scope: SearchScope,
        current_sheet_index: Option<usize>,
    ) -> Result<SearchResponse, AppError> {
        self.indexes.search(
            document_id,
            base_revision,
            query,
            scope,
            current_sheet_index,
        )
    }

    pub fn rebuild_all_sheets_index(&self, document_id: u64) {
        self.indexes.rebuild_all_sheets_index(document_id);
    }

    pub fn schedule_work(&self, document_id: u64, source_revision: u64, work: SearchIndexWork) {
        self.indexes
            .schedule_work(document_id, source_revision, work);
    }

    pub fn cancel_document_jobs(&self, document_id: u64) {
        self.indexes.cancel_document_jobs(document_id);
    }

    #[cfg(test)]
    pub(crate) fn is_isolated_from(&self, other: &Self) -> bool {
        !Arc::ptr_eq(&self.indexes, &other.indexes)
    }
}

#[cfg(test)]
struct NoopSearchIndexPort;

#[cfg(test)]
impl SearchIndexPort for NoopSearchIndexPort {
    fn search(
        &self,
        _document_id: u64,
        _base_revision: u64,
        _query: &str,
        _scope: SearchScope,
        _current_sheet_index: Option<usize>,
    ) -> Result<SearchResponse, AppError> {
        Ok(SearchResponse::default())
    }

    fn rebuild_all_sheets_index(&self, _document_id: u64) {}

    fn schedule_work(&self, _document_id: u64, _source_revision: u64, _work: SearchIndexWork) {}

    fn cancel_document_jobs(&self, _document_id: u64) {}
}
