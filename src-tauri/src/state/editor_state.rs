use crate::document::capabilities::WorkbookCapabilities;
use crate::document::document_memento::{DocumentMemento, DocumentMementoSide};
use crate::document::document_model::SpreadsheetDocument;
use crate::document::document_restore::DocumentRestoreResult;
use crate::document::document_save::SpreadsheetDocumentSaveSnapshot;
use crate::document::formula_coordinator::FormulaWorkLimits;
use crate::document::region_metadata_index::{DocumentRegion, DocumentRegionMetadata};
use crate::document_data::{DocumentData, SheetExtent};
use crate::domain::{
    AppliedOperation, DocumentCellChange, EditorCommand, SearchIndexWork, SearchScanCursor,
    SearchTextChunk,
};
use crate::error::AppError;
use crate::formula::status::FormulaStatus;
use crate::resource_limits::ResourceLedger;
#[cfg(test)]
use crate::state::content_hash::ContentHash;
use crate::state::dirty_tracker::DirtyTracker;
use crate::state::editor_session::EditorSession;
use crate::state::history_store::{
    HistoryEntry, HistoryStatus, HistoryStore, MAX_SINGLE_HISTORY_ENTRY_BYTES,
    RetiredHistoryEntries,
};
#[cfg(test)]
use crate::state::history_store::{MAX_HISTORY_BYTES, MAX_HISTORY_ENTRIES};
use crate::state::search_document::collect_sheet_search_text_chunk;
use std::collections::HashSet;
#[cfg(test)]
use umya_spreadsheet::Workbook;

#[derive(Debug)]
pub struct ExecutedOperation {
    pub operation: Option<AppliedOperation>,
    pub cell_changes: Vec<DocumentCellChange>,
    pub restore: Option<DocumentRestoreResult>,
    pub search_index_work: SearchIndexWork,
    pub(crate) retired: RetiredEditorResources,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveCommitLease {
    document_id: u64,
    revision: u64,
    token: u64,
}

#[derive(Clone, Copy)]
enum HistoryRestoreDirection {
    Undo,
    Redo,
}

/// 编辑器状态管理器
pub struct EditorState {
    session: EditorSession,
    document: SpreadsheetDocument,
    history: HistoryStore,
    dirty: DirtyTracker,
    resources: ResourceLedger,
    resource_estimate_floor: usize,
    save_commit: Option<SaveCommitLease>,
}

#[derive(Default)]
pub(crate) struct RetiredEditorResources {
    _document: Option<SpreadsheetDocument>,
    _history: Option<HistoryStore>,
    _history_entries: RetiredHistoryEntries,
}

impl std::fmt::Debug for RetiredEditorResources {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RetiredEditorResources(..)")
    }
}

impl RetiredEditorResources {
    fn from_history_entries(history_entries: RetiredHistoryEntries) -> Self {
        Self {
            _history_entries: history_entries,
            ..Self::default()
        }
    }

    #[cfg(test)]
    fn retired_history_entry_count(&self) -> usize {
        self._history_entries.len()
    }
}

impl EditorState {
    pub fn new(file_data: DocumentData) -> Self {
        Self::from_document(SpreadsheetDocument::new(file_data))
    }

    pub(crate) fn from_document(document: SpreadsheetDocument) -> Self {
        let dirty = DirtyTracker::new(document.projection());
        let resources = ResourceLedger::from_file_data(document.projection());
        Self {
            session: EditorSession::new(),
            document,
            history: HistoryStore::default(),
            dirty,
            resources,
            resource_estimate_floor: 0,
            save_commit: None,
        }
    }

    pub(crate) fn with_resource_estimate_floor(mut self, estimated_bytes: usize) -> Self {
        self.resource_estimate_floor = estimated_bytes;
        self
    }

    #[cfg(test)]
    pub fn with_workbook(file_data: DocumentData, workbook: Option<Workbook>) -> Self {
        Self::from_document(SpreadsheetDocument::with_workbook(file_data, workbook))
    }

    pub fn file_data(&self) -> &DocumentData {
        self.document.projection()
    }

    #[cfg(test)]
    pub fn update_identity(&mut self, path: String, file_name: String) {
        if self.has_save_commit_in_progress() {
            return;
        }
        self.document.update_identity(path, file_name);
        self.resources.refresh_identity(self.document.projection());
    }

    #[cfg(test)]
    pub(crate) fn rebind_saved_document(
        &mut self,
        file_data: DocumentData,
        workbook: Option<Workbook>,
        clear_history: bool,
    ) -> Result<RetiredEditorResources, AppError> {
        self.rebind_saved_document_model(
            SpreadsheetDocument::with_workbook(file_data, workbook),
            clear_history,
        )
    }

    fn rebind_saved_document_model(
        &mut self,
        document: SpreadsheetDocument,
        clear_history: bool,
    ) -> Result<RetiredEditorResources, AppError> {
        self.ensure_revision_available()?;
        let previous_document = std::mem::replace(&mut self.document, document);
        let previous_history = clear_history.then(|| std::mem::take(&mut self.history));
        self.bump_revision()?;
        self.dirty.replace_current(self.document.projection());
        self.resources.replace_all(self.document.projection());
        Ok(RetiredEditorResources {
            _document: Some(previous_document),
            _history: previous_history,
            ..RetiredEditorResources::default()
        })
    }

    pub fn has_save_commit_in_progress(&self) -> bool {
        self.save_commit.is_some()
    }

    pub fn begin_save_commit(
        &mut self,
        document_id: u64,
        revision: u64,
    ) -> Result<SaveCommitLease, AppError> {
        self.ensure_not_saving()?;
        self.ensure_revision_available()?;
        if self.document_id() != document_id || self.revision() != revision {
            return Err(AppError::DocumentStateInvalid(
                "document changed while save was in progress; please save again".to_string(),
            ));
        }

        let lease = SaveCommitLease {
            document_id,
            revision,
            token: nonzero_random_u64(),
        };
        self.save_commit = Some(lease);
        Ok(lease)
    }

    pub fn abort_save_commit(&mut self, lease: SaveCommitLease) {
        if self.save_commit == Some(lease) {
            self.save_commit = None;
        }
    }

    pub(crate) fn finish_save_commit(
        &mut self,
        lease: SaveCommitLease,
        document: SpreadsheetDocument,
        clear_history: bool,
    ) -> Result<RetiredEditorResources, AppError> {
        if self.save_commit != Some(lease) {
            return Err(AppError::DocumentStateInvalid(
                "save commit lease is no longer active".to_string(),
            ));
        }
        if self.document_id() != lease.document_id || self.revision() != lease.revision {
            self.save_commit = None;
            return Err(AppError::DocumentStateInvalid(
                "document changed while save was in progress; please save again".to_string(),
            ));
        }

        self.save_commit = None;
        let retired = self.rebind_saved_document_model(document, clear_history)?;
        self.mark_saved();
        Ok(retired)
    }

    #[cfg(test)]
    pub fn can_finish_save_without_reparse(&self, target_is_xlsx: bool) -> bool {
        target_is_xlsx && self.document.is_excel_backed()
    }

    pub fn save_snapshot_for_target(
        &self,
        target_path_or_name: &str,
    ) -> Result<SpreadsheetDocumentSaveSnapshot, AppError> {
        self.document.save_snapshot_for_target(target_path_or_name)
    }

    pub fn is_csv_backed(&self) -> bool {
        self.document.is_csv_backed()
    }

    pub(crate) fn finish_save_commit_without_reparse(
        &mut self,
        lease: SaveCommitLease,
        path: String,
        file_name: String,
        clear_history: bool,
    ) -> Result<RetiredEditorResources, AppError> {
        if self.save_commit != Some(lease) {
            return Err(AppError::DocumentStateInvalid(
                "save commit lease is no longer active".to_string(),
            ));
        }
        if self.document_id() != lease.document_id || self.revision() != lease.revision {
            self.save_commit = None;
            return Err(AppError::DocumentStateInvalid(
                "document changed while save was in progress; please save again".to_string(),
            ));
        }
        self.ensure_revision_available()?;

        self.save_commit = None;
        self.document.update_identity(path, file_name);
        self.resources.refresh_identity(self.document.projection());
        let previous_history = clear_history.then(|| std::mem::take(&mut self.history));
        self.bump_revision()?;
        self.mark_saved();
        Ok(RetiredEditorResources {
            _document: None,
            _history: previous_history,
            ..RetiredEditorResources::default()
        })
    }

    pub fn document_id(&self) -> u64 {
        self.session.document_id()
    }

    pub fn revision(&self) -> u64 {
        self.session.revision()
    }

    pub fn can_undo(&self) -> bool {
        self.transaction_failure().is_none() && self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.transaction_failure().is_none() && self.history.can_redo()
    }

    pub fn history_status(&self) -> HistoryStatus {
        self.history.status()
    }

    #[cfg(test)]
    pub fn current_content_hash(&self) -> ContentHash {
        self.dirty.current_hash()
    }

    pub fn formula_status(&self) -> FormulaStatus {
        self.document.formula_status()
    }

    pub fn capabilities(&self) -> WorkbookCapabilities {
        self.document.capabilities()
    }

    pub fn sheet_extents(&self) -> Vec<SheetExtent> {
        self.resources.sheet_extents()
    }

    pub fn sheet_extent(&self, sheet_index: usize) -> Option<SheetExtent> {
        self.resources.sheet_extent(sheet_index)
    }

    pub fn region_metadata(&self, region: &DocumentRegion) -> DocumentRegionMetadata {
        self.document.region_metadata(region)
    }

    pub fn estimated_resource_bytes(&self) -> usize {
        self.resources
            .estimated_bytes()
            .saturating_add(self.document.estimated_runtime_bytes())
            .saturating_add(self.history.estimated_bytes())
            .max(self.resource_estimate_floor)
    }

    pub fn transaction_failure(&self) -> Option<&str> {
        self.document.transaction_failure()
    }

    pub fn search_sheet_text_chunk(
        &self,
        sheet_index: usize,
        cursor: SearchScanCursor,
        maximum_text_bytes: usize,
        maximum_cells: usize,
    ) -> Option<SearchTextChunk> {
        self.file_data().sheets.get(sheet_index).map(|sheet| {
            collect_sheet_search_text_chunk(sheet, cursor, maximum_text_bytes, maximum_cells)
        })
    }

    pub fn search_sheet_snapshot_estimated_bytes(&self, sheet_index: usize) -> Option<usize> {
        self.resources.sheet_estimated_bytes(sheet_index)
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty.is_dirty()
    }

    pub fn mark_saved(&mut self) {
        self.dirty.mark_saved();
    }

    #[cfg(test)]
    pub fn generate_file_bytes_for_target(
        &self,
        target_path_or_name: &str,
    ) -> Result<(String, Vec<u8>), AppError> {
        self.document
            .generate_file_bytes_for_target(target_path_or_name)
    }

    /// 执行命令并记录到历史，返回增量结果。
    pub fn execute(&mut self, command: EditorCommand) -> Result<ExecutedOperation, AppError> {
        self.execute_with_formula_work_limits(command, FormulaWorkLimits::default())
    }

    fn execute_with_formula_work_limits(
        &mut self,
        command: EditorCommand,
        formula_work_limits: FormulaWorkLimits,
    ) -> Result<ExecutedOperation, AppError> {
        self.ensure_not_saving()?;
        self.ensure_transaction_available()?;
        let operation = command.resolve_with_resources(self.file_data(), &self.resources)?;
        if operation.impact().is_noop() {
            return Ok(ExecutedOperation {
                operation: None,
                cell_changes: Vec::new(),
                restore: None,
                search_index_work: SearchIndexWork::None,
                retired: RetiredEditorResources::default(),
            });
        }
        self.ensure_operation_supported(&operation)?;
        self.document
            .validate_formula_work(&operation, formula_work_limits)?;
        self.ensure_memento_budget(&operation)?;
        self.ensure_revision_available()?;
        let should_mark_search_stale = operation.impact().requires_search_rebuild();
        let before = self.document.capture_memento_side(&operation);

        let result = self.document.execute_operation(&operation, &before)?;
        let cell_changes = result.cell_changes;
        let resource_sheets = operation_resource_sheets(&operation, &cell_changes);
        self.resources
            .refresh_sheets(self.document.projection(), resource_sheets);
        self.dirty
            .apply_operation(&operation, &cell_changes, self.document.projection());

        let retired_history = if before.estimated_bytes() > MAX_SINGLE_HISTORY_ENTRY_BYTES {
            self.history.clear_all()
        } else {
            let after = self.document.capture_memento_side(&operation);
            let memento = SpreadsheetDocument::create_memento(before, after);
            let entry = HistoryEntry::new(memento);
            self.history.record(entry)
        };

        self.bump_revision()?;
        if should_mark_search_stale {
            return Ok(ExecutedOperation {
                operation: Some(operation),
                cell_changes,
                restore: None,
                search_index_work: SearchIndexWork::RebuildAll,
                retired: RetiredEditorResources::from_history_entries(retired_history),
            });
        }
        Ok(ExecutedOperation {
            operation: Some(operation),
            cell_changes,
            restore: None,
            search_index_work: SearchIndexWork::None,
            retired: RetiredEditorResources::from_history_entries(retired_history),
        })
    }

    /// 撤销上一个操作
    pub fn undo(&mut self) -> Result<Option<ExecutedOperation>, AppError> {
        self.restore_history(HistoryRestoreDirection::Undo)
    }

    /// 重做上一个被撤销的操作
    pub fn redo(&mut self) -> Result<Option<ExecutedOperation>, AppError> {
        self.restore_history(HistoryRestoreDirection::Redo)
    }

    fn restore_history(
        &mut self,
        direction: HistoryRestoreDirection,
    ) -> Result<Option<ExecutedOperation>, AppError> {
        self.ensure_not_saving()?;
        self.ensure_transaction_available()?;
        self.ensure_revision_available()?;
        let entry = match direction {
            HistoryRestoreDirection::Undo => self.history.peek_undo(),
            HistoryRestoreDirection::Redo => self.history.peek_redo(),
        };
        let Some(entry) = entry else {
            return Ok(None);
        };
        let (target, rollback) = history_restore_sides(&entry.memento, direction);
        let restore = match self.document.restore_memento_side(target) {
            Ok(restore) => restore,
            Err(error) => {
                rollback_failed_history_restore(&mut self.document, rollback, &error)?;
                return Err(error);
            }
        };

        self.dirty
            .apply_history_restore(target, rollback, self.document.projection());
        let retired_history = match direction {
            HistoryRestoreDirection::Undo => {
                let entry = self
                    .history
                    .pop_undo()
                    .expect("undo entry exists until history restore commits");
                self.history.push_redo(entry)
            }
            HistoryRestoreDirection::Redo => {
                let entry = self
                    .history
                    .pop_redo()
                    .expect("redo entry exists until history restore commits");
                self.history.push_undo(entry)
            }
        };
        self.resources.replace_all(self.document.projection());
        self.bump_revision()?;

        Ok(Some(ExecutedOperation {
            operation: None,
            cell_changes: Vec::new(),
            restore: Some(restore),
            search_index_work: SearchIndexWork::RebuildAll,
            retired: RetiredEditorResources::from_history_entries(retired_history),
        }))
    }

    #[cfg(test)]
    fn refresh_content_hash(&mut self) {
        self.dirty.replace_current(self.document.projection());
    }

    fn bump_revision(&mut self) -> Result<(), AppError> {
        self.session.bump_revision().ok_or_else(|| {
            AppError::DocumentStateInvalid("document revision space exhausted".to_string())
        })?;
        Ok(())
    }

    fn ensure_revision_available(&self) -> Result<(), AppError> {
        if self.session.can_bump_revision() {
            return Ok(());
        }
        Err(AppError::DocumentStateInvalid(
            "document revision space exhausted".to_string(),
        ))
    }

    fn ensure_operation_supported(&mut self, operation: &AppliedOperation) -> Result<(), AppError> {
        if let Some(reason) = self.transaction_failure() {
            return Err(AppError::DocumentStateInvalid(reason.to_string()));
        }
        let unsupported = self.document.unsupported_operation_features(operation);
        if !unsupported.is_empty() {
            return Err(AppError::UnsupportedWorkbookStructure(
                unsupported.join(", "),
            ));
        }
        Ok(())
    }

    fn ensure_transaction_available(&self) -> Result<(), AppError> {
        if let Some(reason) = self.transaction_failure() {
            return Err(AppError::DocumentStateInvalid(reason.to_string()));
        }
        Ok(())
    }

    fn ensure_not_saving(&self) -> Result<(), AppError> {
        if self.save_commit.is_some() {
            return Err(AppError::DocumentStateInvalid(
                "save is already in progress".to_string(),
            ));
        }
        Ok(())
    }

    fn ensure_memento_budget(&mut self, operation: &AppliedOperation) -> Result<(), AppError> {
        let estimated_bytes = self.document.estimate_memento_side_bytes(operation);
        if estimated_bytes > MAX_SINGLE_HISTORY_ENTRY_BYTES {
            return Err(AppError::DocumentStateInvalid(format!(
                "operation is too large for safe undo/rollback history (estimated {estimated_bytes} bytes, limit {MAX_SINGLE_HISTORY_ENTRY_BYTES} bytes)"
            )));
        }
        Ok(())
    }
}

fn rollback_failed_history_restore(
    document: &mut SpreadsheetDocument,
    rollback: &DocumentMementoSide,
    restore_error: &AppError,
) -> Result<(), AppError> {
    match document.restore_memento_side(rollback) {
        Ok(_) => Ok(()),
        Err(rollback_error) => {
            let operation_error = restore_error.to_string();
            let rollback_error = rollback_error.to_string();
            document.mark_transaction_failed(format!(
                "history restore failed ({operation_error}) and rollback failed ({rollback_error})"
            ));
            Err(AppError::TransactionRollbackFailed {
                operation_error,
                rollback_error,
            })
        }
    }
}

fn history_restore_sides(
    memento: &DocumentMemento,
    direction: HistoryRestoreDirection,
) -> (&DocumentMementoSide, &DocumentMementoSide) {
    match direction {
        HistoryRestoreDirection::Undo => (&memento.before, &memento.after),
        HistoryRestoreDirection::Redo => (&memento.after, &memento.before),
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

fn operation_resource_sheets(
    operation: &AppliedOperation,
    formula_changes: &[DocumentCellChange],
) -> Vec<usize> {
    let mut sheets = HashSet::new();
    match operation {
        AppliedOperation::SetCell { sheet_index, .. }
        | AppliedOperation::SetColumnWidth { sheet_index, .. }
        | AppliedOperation::SetRowHeight { sheet_index, .. }
        | AppliedOperation::AddRow { sheet_index, .. }
        | AppliedOperation::DeleteRow { sheet_index, .. }
        | AppliedOperation::AddColumn { sheet_index, .. }
        | AppliedOperation::DeleteColumn { sheet_index, .. }
        | AppliedOperation::DeleteSheet { sheet_index } => {
            sheets.insert(*sheet_index);
        }
        AppliedOperation::SetCells { changes } => {
            sheets.extend(changes.iter().map(|change| change.sheet_index));
        }
        AppliedOperation::AddSheet { sheet_index, .. } => {
            sheets.insert(*sheet_index);
        }
    }
    sheets.extend(formula_changes.iter().map(|change| change.sheet_index));
    sheets.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use crate::document_data::{CellFormat, DocumentSheet, RichMetadata};
    use std::collections::HashMap;
    use std::io::Cursor;

    use super::*;
    use crate::document::region_metadata_index::DocumentRegion;
    use crate::document::test_support::read_file_with_workbook_from_bytes;
    use crate::domain::{CellNumber, CellValue, EditorCommand};
    use umya_spreadsheet::{Color, DefinedName, SheetProtection, reader, writer};

    fn assert_incremental_content_hash_is_current(state: &EditorState) {
        let rebuilt = crate::state::content_hash::hash_content_fingerprint(
            &crate::state::content_hash::ContentFingerprint::from_file_data(state.file_data()),
        );
        assert_eq!(state.current_content_hash(), rebuilt);
    }

    #[test]
    fn updating_file_identity_does_not_mark_content_dirty() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: "/tmp/source.xlsx".to_string(),
                file_name: "source.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::String("value".to_string())]],
                    ..Default::default()
                }],
            },
            None,
        );
        let original_hash = state.current_content_hash();

        state.update_identity("/tmp/renamed.xlsx".to_string(), "renamed.xlsx".to_string());
        state.refresh_content_hash();

        assert_eq!(state.file_data().path, "/tmp/renamed.xlsx");
        assert_eq!(state.file_data().file_name, "renamed.xlsx");
        assert_eq!(state.current_content_hash(), original_hash);
        assert!(!state.is_dirty());
    }

    #[test]
    fn incremental_content_hash_tracks_cells_layout_and_history() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "incremental.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::String("saved".to_string())]],
                    ..Default::default()
                }],
            },
            None,
        );
        let saved_hash = state.current_content_hash();

        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 3,
                col: 4,
                text: "far".to_string(),
            })
            .expect("extend projection");
        assert_incremental_content_hash_is_current(&state);
        assert!(state.is_dirty());

        state.undo().expect("undo far edit").expect("undo result");
        assert_incremental_content_hash_is_current(&state);
        assert_eq!(state.current_content_hash(), saved_hash);
        assert!(!state.is_dirty());

        state.redo().expect("redo far edit").expect("redo result");
        assert_incremental_content_hash_is_current(&state);

        state
            .execute(EditorCommand::SetColumnWidth {
                sheet_index: 0,
                col_index: 4,
                width: Some(180),
            })
            .expect("resize column");
        assert_incremental_content_hash_is_current(&state);
        state.undo().expect("undo resize").expect("undo result");
        assert_incremental_content_hash_is_current(&state);
    }

    #[test]
    fn incremental_content_hash_returns_clean_when_saved_value_is_restored() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "incremental.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::String("saved".to_string())]],
                    ..Default::default()
                }],
            },
            None,
        );

        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "changed".to_string(),
            })
            .expect("change value");
        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "saved".to_string(),
            })
            .expect("restore saved value");

        assert_incremental_content_hash_is_current(&state);
        assert!(!state.is_dirty());
    }

    #[test]
    fn formula_work_limit_rejects_before_document_state_changes() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "formula-work-limit.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![
                        CellValue::String("1".to_string()),
                        CellValue::formula("=A1", CellValue::Null),
                        CellValue::formula("=A1+1", CellValue::Null),
                    ]],
                    ..Default::default()
                }],
            },
            None,
        );
        let original_revision = state.revision();
        let original_file_data = state.file_data().clone();

        let error = state
            .execute_with_formula_work_limits(
                EditorCommand::SetCell {
                    sheet_index: 0,
                    row: 0,
                    col: 0,
                    text: "2".to_string(),
                },
                FormulaWorkLimits {
                    max_evaluations: 1,
                    max_source_bytes: usize::MAX,
                },
            )
            .expect_err("formula work should exceed the test limit");

        assert!(matches!(error, AppError::ResourceLimitExceeded(_)));
        assert_eq!(state.revision(), original_revision);
        assert_eq!(state.file_data(), &original_file_data);
        assert!(!state.is_dirty());
        assert!(!state.can_undo());
    }

    #[test]
    fn revision_exhaustion_rejects_mutation_before_state_changes() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "revision-limit.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::String("saved".to_string())]],
                    ..Default::default()
                }],
            },
            None,
        );
        state.session.set_revision_for_test(u64::MAX);

        let error = state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "changed".to_string(),
            })
            .expect_err("revision exhaustion must reject the mutation");

        assert!(matches!(error, AppError::DocumentStateInvalid(_)));
        assert_eq!(
            state.file_data().sheets[0].rows[0][0],
            CellValue::String("saved".to_string())
        );
        assert!(!state.can_undo());
        assert!(!state.is_dirty());
    }

    #[test]
    fn incremental_content_hash_rebuilds_only_structurally_affected_state() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "incremental.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::String("value".to_string())]],
                    ..Default::default()
                }],
            },
            None,
        );

        for command in [
            EditorCommand::AddRow {
                sheet_index: 0,
                row_index: 1,
            },
            EditorCommand::AddColumn {
                sheet_index: 0,
                col_index: 1,
            },
            EditorCommand::AddSheet {
                name: Some("Second".to_string()),
            },
            EditorCommand::DeleteSheet { sheet_index: 1 },
        ] {
            state.execute(command).expect("structure edit");
            assert_incremental_content_hash_is_current(&state);
            state.undo().expect("undo structure").expect("undo result");
            assert_incremental_content_hash_is_current(&state);
            state.redo().expect("redo structure").expect("redo result");
            assert_incremental_content_hash_is_current(&state);
        }
    }

    #[test]
    fn structure_edits_refresh_region_metadata_index_for_undo_and_redo() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "metadata.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::Null], vec![CellValue::Null]],
                    rich: RichMetadata {
                        cell_formats: HashMap::from([(
                            "A2".to_string(),
                            CellFormat {
                                number_format: Some("0%".to_string()),
                                style_id: None,
                            },
                        )]),
                        ..Default::default()
                    },
                    ..Default::default()
                }],
            },
            None,
        );

        state
            .execute(EditorCommand::AddRow {
                sheet_index: 0,
                row_index: 0,
            })
            .expect("add row");
        assert!(region_formats(&state, 2).contains_key("A3"));

        state.undo().expect("undo").expect("undo result");
        assert!(region_formats(&state, 1).contains_key("A2"));

        state.redo().expect("redo").expect("redo result");
        assert!(region_formats(&state, 2).contains_key("A3"));
    }

    fn region_formats(state: &EditorState, row: usize) -> HashMap<String, CellFormat> {
        state
            .region_metadata(&DocumentRegion {
                sheet_index: 0,
                row_start: row,
                row_end: row + 1,
                col_start: 0,
                col_end: 1,
            })
            .cell_formats
    }

    #[test]
    fn incremental_content_hash_tracks_cross_sheet_formula_rewrites() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "incremental.xlsx".to_string(),
                sheets: vec![
                    DocumentSheet {
                        name: "Data".to_string(),
                        rows: vec![vec![CellValue::Number(1.into())]],
                        ..Default::default()
                    },
                    DocumentSheet {
                        name: "Summary".to_string(),
                        rows: vec![vec![CellValue::formula(
                            "=Data!A1",
                            CellValue::Number(1.into()),
                        )]],
                        ..Default::default()
                    },
                ],
            },
            None,
        );

        state
            .execute(EditorCommand::AddRow {
                sheet_index: 0,
                row_index: 0,
            })
            .expect("insert referenced row");
        assert_incremental_content_hash_is_current(&state);

        state
            .undo()
            .expect("undo referenced row")
            .expect("undo result");
        assert_incremental_content_hash_is_current(&state);
        state
            .redo()
            .expect("redo referenced row")
            .expect("redo result");
        assert_incremental_content_hash_is_current(&state);
    }

    #[test]
    fn save_commit_lease_blocks_mutations_until_released() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: "/tmp/source.xlsx".to_string(),
                file_name: "source.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::String("old".to_string())]],
                    ..Default::default()
                }],
            },
            None,
        );
        let lease = state
            .begin_save_commit(state.document_id(), state.revision())
            .expect("begin save commit");

        let error = state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "new".to_string(),
            })
            .expect_err("mutation should be blocked while save is in progress");

        assert!(error.to_string().contains("save is already in progress"));

        state.abort_save_commit(lease);
        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "new".to_string(),
            })
            .expect("mutation after save lease release");
    }

    #[test]
    fn failed_undo_rolls_back_document_and_keeps_history_entry() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "history.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::String("old".to_string())]],
                    ..Default::default()
                }],
            },
            None,
        );
        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "new".to_string(),
            })
            .expect("edit");
        let revision = state.revision();
        let content_hash = state.current_content_hash();
        state.document.inject_restore_failures(1);

        let error = state.undo().expect_err("injected undo failure");

        assert!(
            error
                .to_string()
                .contains("injected history restore failure")
        );
        assert_eq!(
            state.file_data().sheets[0].rows[0][0],
            CellValue::String("new".to_string())
        );
        assert_eq!(state.revision(), revision);
        assert_eq!(state.current_content_hash(), content_hash);
        assert!(state.can_undo());
        assert!(!state.can_redo());
        assert!(state.transaction_failure().is_none());

        state.undo().expect("retry undo").expect("undo result");
        assert_eq!(
            state.file_data().sheets[0].rows[0][0],
            CellValue::String("old".to_string())
        );
    }

    #[test]
    fn failed_redo_rolls_back_document_and_keeps_history_entry() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "history.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::String("old".to_string())]],
                    ..Default::default()
                }],
            },
            None,
        );
        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "new".to_string(),
            })
            .expect("edit");
        state.undo().expect("undo").expect("undo result");
        let revision = state.revision();
        state.document.inject_restore_failures(1);

        state.redo().expect_err("injected redo failure");

        assert_eq!(
            state.file_data().sheets[0].rows[0][0],
            CellValue::String("old".to_string())
        );
        assert_eq!(state.revision(), revision);
        assert!(!state.can_undo());
        assert!(state.can_redo());
        assert!(state.transaction_failure().is_none());

        state.redo().expect("retry redo").expect("redo result");
        assert_eq!(
            state.file_data().sheets[0].rows[0][0],
            CellValue::String("new".to_string())
        );
    }

    #[test]
    fn failed_undo_after_workbook_patch_restores_workbook_and_history() {
        let mut source = umya_spreadsheet::new_file();
        source
            .sheet_mut(0)
            .expect("sheet")
            .cell_mut("A1")
            .set_value_string("old");
        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "history-post-patch.xlsx".to_string(),
        )
        .expect("read source");
        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "new".to_string(),
            })
            .expect("edit");
        let revision = state.revision();
        let content_hash = state.current_content_hash();
        state.document.inject_post_patch_restore_failures(1);

        let error = state.undo().expect_err("injected post-patch failure");

        assert!(
            error
                .to_string()
                .contains("injected post-patch history restore failure")
        );
        assert_eq!(state.revision(), revision);
        assert_eq!(state.current_content_hash(), content_hash);
        assert!(state.can_undo());
        assert!(!state.can_redo());
        assert_eq!(
            state.file_data().sheets[0].rows[0][0],
            CellValue::String("new".to_string())
        );
        let (_, saved_bytes) = state
            .generate_file_bytes_for_target("history-post-patch.xlsx")
            .expect("save rolled-back workbook");
        let saved = reader::xlsx::read_reader(Cursor::new(saved_bytes), true)
            .expect("read rolled-back workbook");
        assert_eq!(
            saved
                .sheet(0)
                .expect("sheet")
                .cell("A1")
                .expect("A1")
                .value(),
            "new"
        );

        state.undo().expect("retry undo").expect("undo result");
        assert_eq!(
            state.file_data().sheets[0].rows[0][0],
            CellValue::String("old".to_string())
        );
    }

    #[test]
    fn failed_history_rollback_marks_document_unavailable() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "history.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::String("old".to_string())]],
                    ..Default::default()
                }],
            },
            None,
        );
        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "new".to_string(),
            })
            .expect("edit");
        let revision = state.revision();
        state.document.inject_restore_failures(2);

        let error = state
            .undo()
            .expect_err("injected undo and rollback failure");

        assert!(matches!(error, AppError::TransactionRollbackFailed { .. }));
        assert_eq!(state.revision(), revision);
        assert!(!state.can_undo());
        assert!(!state.can_redo());
        assert_eq!(state.history_status().undo_entries, 1);
        assert!(state.transaction_failure().is_some());
        assert!(!state.capabilities().save.can_native_save);
        assert!(state.undo().is_err());
        assert!(
            state
                .execute(EditorCommand::SetCell {
                    sheet_index: 0,
                    row: 0,
                    col: 0,
                    text: "blocked".to_string(),
                })
                .is_err()
        );
    }

    #[test]
    fn excel_backed_save_can_finish_without_reparse() {
        let mut source = umya_spreadsheet::new_file();
        source
            .sheet_mut(0)
            .expect("sheet")
            .cell_mut("A1")
            .set_value_string("old");
        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            "/tmp/source.xlsx".to_string(),
            "source.xlsx".to_string(),
        )
        .expect("read source");
        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);

        assert!(state.can_finish_save_without_reparse(true));
        assert!(!state.can_finish_save_without_reparse(false));

        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "new".to_string(),
            })
            .expect("edit");
        assert!(state.is_dirty());
        let revision_before_save = state.revision();
        let lease = state
            .begin_save_commit(state.document_id(), state.revision())
            .expect("begin save commit");

        state
            .finish_save_commit_without_reparse(
                lease,
                "/tmp/saved.xlsx".to_string(),
                "saved.xlsx".to_string(),
                false,
            )
            .expect("finish save");

        assert_eq!(state.file_data().path, "/tmp/saved.xlsx");
        assert_eq!(state.file_data().file_name, "saved.xlsx");
        assert_eq!(state.revision(), revision_before_save + 1);
        assert!(!state.is_dirty());
        assert!(state.can_undo());
    }

    #[test]
    fn opened_workbook_is_patched_and_saved_from_editor_state() {
        let mut source = umya_spreadsheet::new_file();
        {
            let sheet = source.sheet_mut(0).expect("sheet");
            sheet.cell_mut("A1").set_value_string("old");
            sheet
                .cell_mut("A1")
                .style_mut()
                .set_background_color(Color::COLOR_RED_STR);
            sheet.cell_mut("B1").set_formula("A1");
            sheet.cell_mut("B1").set_formula_result_string("old");
        }

        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "styled.xlsx".to_string(),
        )
        .expect("read source");

        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "42".to_string(),
            })
            .expect("set cell");

        let (_, saved_bytes) = state
            .generate_file_bytes_for_target("styled.xlsx")
            .expect("save from workbook");
        let saved = reader::xlsx::read_reader(Cursor::new(saved_bytes), true).expect("read saved");
        let sheet = saved.sheet(0).expect("saved sheet");

        assert_eq!(sheet.cell("A1").expect("A1").value(), "42");
        assert_eq!(
            sheet
                .cell("A1")
                .expect("A1")
                .style()
                .background_color()
                .map(|color| color.argb_str()),
            Some(Color::COLOR_RED_STR.to_string())
        );
        assert!(sheet.cell("B1").expect("B1").cell_value().is_formula());
    }

    #[test]
    fn row_column_undo_redo_patch_saved_workbook() {
        let mut source = umya_spreadsheet::new_file();
        {
            let sheet = source.sheet_mut(0).expect("sheet");
            sheet.cell_mut("A1").set_value_string("a1");
            sheet.cell_mut("B1").set_value_string("b1");
            sheet.cell_mut("A2").set_value_string("a2");
            sheet.cell_mut("B2").set_value_string("b2");
        }

        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "structure.xlsx".to_string(),
        )
        .expect("read source");

        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        state
            .execute(EditorCommand::DeleteRow {
                sheet_index: 0,
                row_index: 0,
            })
            .expect("delete row");
        state.undo().expect("undo row delete").expect("undo result");
        state.redo().expect("redo row delete").expect("redo result");
        state
            .execute(EditorCommand::DeleteColumn {
                sheet_index: 0,
                col_index: 0,
            })
            .expect("delete column");
        state
            .undo()
            .expect("undo column delete")
            .expect("undo result");

        let (_, saved_bytes) = state
            .generate_file_bytes_for_target("structure.xlsx")
            .expect("save from workbook");
        let saved = reader::xlsx::read_reader(Cursor::new(saved_bytes), true).expect("read saved");
        let sheet = saved.sheet(0).expect("saved sheet");

        assert_eq!(sheet.cell("A1").expect("A1").value(), "a2");
        assert_eq!(sheet.cell("B1").expect("B1").value(), "b2");
        assert!(sheet.cell("A2").is_none());
    }

    #[test]
    fn workbook_structure_patch_preserves_adjusted_formula_references() {
        let mut source = umya_spreadsheet::new_file();
        {
            let sheet = source.sheet_mut(0).expect("sheet");
            sheet.cell_mut("A1").set_value_number(1);
            sheet.cell_mut("A2").set_value_number(2);
            sheet.cell_mut("B2").set_formula("SUM(A1:A2)");
            sheet.cell_mut("B2").set_formula_result_number(3.0);
        }

        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "formula-shift.xlsx".to_string(),
        )
        .expect("read source");

        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        state
            .execute(EditorCommand::AddRow {
                sheet_index: 0,
                row_index: 1,
            })
            .expect("add row");

        let (_, saved_bytes) = state
            .generate_file_bytes_for_target("formula-shift.xlsx")
            .expect("save from workbook");
        let saved = reader::xlsx::read_reader(Cursor::new(saved_bytes), true).expect("read saved");
        let sheet = saved.sheet(0).expect("saved sheet");

        assert_eq!(
            sheet.cell("B3").expect("B3").formula(),
            "SUM(A1:A3)",
            "formula references should come from workbook structure adjustment"
        );
        match &state.file_data().sheets[0].rows[2][1] {
            CellValue::Formula { formula, .. } => assert_eq!(formula, "=SUM(A1:A3)"),
            value => panic!("expected adjusted formula in projection, got {value:?}"),
        }
    }

    #[test]
    fn workbook_structure_patch_preserves_explicit_same_sheet_formula_references() {
        let mut source = umya_spreadsheet::new_file();
        {
            let sheet = source.sheet_mut(0).expect("sheet");
            sheet.set_name("Inputs");
            sheet.cell_mut("A1").set_value_number(1);
            sheet.cell_mut("B1").set_formula("inputs!A1");
            sheet.cell_mut("B1").set_formula_result_number(1.0);
        }

        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "same-sheet-explicit-formula.xlsx".to_string(),
        )
        .expect("read source");

        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        state
            .execute(EditorCommand::AddRow {
                sheet_index: 0,
                row_index: 0,
            })
            .expect("add row");

        match &state.file_data().sheets[0].rows[1][1] {
            CellValue::Formula { formula, .. } => assert_eq!(formula, "=inputs!A2"),
            value => panic!("expected adjusted explicit same-sheet formula, got {value:?}"),
        }

        let (_, saved_bytes) = state
            .generate_file_bytes_for_target("same-sheet-explicit-formula.xlsx")
            .expect("save workbook");
        let saved = reader::xlsx::read_reader(Cursor::new(saved_bytes), true).expect("read saved");
        assert_eq!(
            saved
                .sheet(0)
                .expect("sheet")
                .cell("B2")
                .expect("B2")
                .formula(),
            "inputs!A2"
        );
    }

    #[test]
    fn workbook_structure_patch_refreshes_cross_sheet_formula_projection() {
        let mut source = umya_spreadsheet::new_file();
        source.new_sheet("Other").expect("other sheet");
        {
            let inputs = source.sheet_mut(0).expect("input sheet");
            inputs.set_name("Inputs");
            inputs.cell_mut("A1").set_value_number(1);
            inputs.cell_mut("A2").set_value_number(2);
        }
        {
            let other = source.sheet_mut(1).expect("other sheet");
            other.cell_mut("A1").set_formula("inputs!A2");
            other.cell_mut("A1").set_formula_result_number(2.0);
        }

        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "cross-sheet-formula.xlsx".to_string(),
        )
        .expect("read source");

        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        state
            .execute(EditorCommand::AddRow {
                sheet_index: 0,
                row_index: 0,
            })
            .expect("add row");

        match &state.file_data().sheets[1].rows[0][0] {
            CellValue::Formula { formula, .. } => assert_eq!(formula, "=inputs!A3"),
            value => panic!("expected adjusted cross-sheet formula, got {value:?}"),
        }

        let (_, saved_bytes) = state
            .generate_file_bytes_for_target("cross-sheet-formula.xlsx")
            .expect("save from workbook");
        let saved = reader::xlsx::read_reader(Cursor::new(saved_bytes), true).expect("read saved");

        assert_eq!(
            saved
                .sheet(1)
                .expect("sheet")
                .cell("A1")
                .expect("A1")
                .formula(),
            "inputs!A3"
        );
    }

    #[test]
    fn unparseable_formulas_block_structure_edits() {
        let mut source = umya_spreadsheet::new_file();
        source.new_sheet("Other").expect("other sheet");
        {
            let inputs = source.sheet_mut(0).expect("input sheet");
            inputs.set_name("Inputs");
            inputs.cell_mut("A1").set_value_number(1);
        }
        {
            let other = source.sheet_mut(1).expect("other sheet");
            other.cell_mut("A1").set_formula("SUM(");
            other.cell_mut("A1").set_formula_result_string("#VALUE!");
        }

        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "skipped-rewrite.xlsx".to_string(),
        )
        .expect("read source");

        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        let error = state
            .execute(EditorCommand::AddRow {
                sheet_index: 0,
                row_index: 0,
            })
            .expect_err("structure edits must be blocked");

        assert!(matches!(
            error,
            AppError::UnsupportedWorkbookStructure(reason) if reason.contains("unparseable formulas")
        ));
        let capabilities = state.capabilities();
        assert!(!capabilities.sheets[0].can_insert_delete_rows);
        assert!(!capabilities.sheets[0].can_insert_delete_columns);
        assert!(!capabilities.structure.can_insert_delete_sheets);
    }

    #[test]
    fn formula_edits_refresh_workbook_capabilities_and_undo_restores_them() {
        let mut source = umya_spreadsheet::new_file();
        source
            .sheet_mut(0)
            .expect("sheet")
            .cell_mut("A1")
            .set_value_number(1);

        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "capabilities.xlsx".to_string(),
        )
        .expect("read source");

        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        assert!(state.capabilities().sheets[0].can_insert_delete_rows);

        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 1,
                text: "=SUM(".to_string(),
            })
            .expect("invalid formula edit is isolated to the cell");

        let capabilities = state.capabilities();
        assert!(!capabilities.sheets[0].can_insert_delete_rows);
        assert!(
            capabilities
                .structure
                .blocked_structure_reasons
                .contains(&"unparseable formulas".to_string())
        );

        state
            .undo()
            .expect("undo formula edit")
            .expect("undo result");
        assert!(state.capabilities().sheets[0].can_insert_delete_rows);
        state
            .execute(EditorCommand::AddRow {
                sheet_index: 0,
                row_index: 1,
            })
            .expect("structure edit after formula undo");
    }

    #[test]
    fn structure_undo_redo_restores_cross_sheet_formula_rewrites() {
        let mut source = umya_spreadsheet::new_file();
        source.new_sheet("Other").expect("other sheet");
        {
            let inputs = source.sheet_mut(0).expect("input sheet");
            inputs.set_name("Inputs");
            inputs.cell_mut("A1").set_value_number(1);
            inputs.cell_mut("A2").set_value_number(2);
        }
        {
            let other = source.sheet_mut(1).expect("other sheet");
            other.cell_mut("A1").set_formula("Inputs!A2");
            other.cell_mut("A1").set_formula_result_number(2.0);
        }

        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "cross-sheet-formula-undo.xlsx".to_string(),
        )
        .expect("read source");

        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        state
            .execute(EditorCommand::AddRow {
                sheet_index: 0,
                row_index: 0,
            })
            .expect("add row");
        match &state.file_data().sheets[1].rows[0][0] {
            CellValue::Formula { formula, .. } => assert_eq!(formula, "=Inputs!A3"),
            value => panic!("expected adjusted formula, got {value:?}"),
        }

        state.undo().expect("undo add row").expect("undo result");
        match &state.file_data().sheets[1].rows[0][0] {
            CellValue::Formula { formula, .. } => assert_eq!(formula, "=Inputs!A2"),
            value => panic!("expected restored formula, got {value:?}"),
        }

        state.redo().expect("redo add row").expect("redo result");
        match &state.file_data().sheets[1].rows[0][0] {
            CellValue::Formula { formula, .. } => assert_eq!(formula, "=Inputs!A3"),
            value => panic!("expected adjusted formula after redo, got {value:?}"),
        }

        let (_, saved_bytes) = state
            .generate_file_bytes_for_target("cross-sheet-formula-undo.xlsx")
            .expect("save workbook");
        let saved = reader::xlsx::read_reader(Cursor::new(saved_bytes), true).expect("read saved");
        assert_eq!(
            saved
                .sheet(1)
                .expect("sheet")
                .cell("A1")
                .expect("A1")
                .formula(),
            "Inputs!A3"
        );
    }

    #[test]
    fn structure_patch_refreshes_projection_for_merges_layout_and_formulas() {
        let mut source = umya_spreadsheet::new_file();
        {
            let sheet = source.sheet_mut(0).expect("sheet");
            sheet.cell_mut("A1").set_value_number(1);
            sheet.cell_mut("A2").set_value_number(2);
            sheet.cell_mut("B2").set_formula("SUM(A1:A2)");
            sheet.cell_mut("B2").set_formula_result_number(3.0);
            sheet.add_merge_cells("C1:D2");
            sheet.row_dimension_mut(1).set_height(84.0);
            sheet.column_dimension_by_number_mut(3).set_width(25.0);
        }

        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "projection-refresh.xlsx".to_string(),
        )
        .expect("read source");

        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        state
            .execute(EditorCommand::AddRow {
                sheet_index: 0,
                row_index: 0,
            })
            .expect("add row");

        let sheet = &state.file_data().sheets[0];
        assert_eq!(
            sheet
                .row_heights
                .as_ref()
                .and_then(|heights| heights.get(&1)),
            Some(&112)
        );
        assert_eq!(
            sheet
                .column_widths
                .as_ref()
                .and_then(|widths| widths.get(&2)),
            Some(&180)
        );
        assert_eq!(sheet.merges.len(), 1);
        assert_eq!(sheet.merges[0].start_row, 1);
        assert_eq!(sheet.merges[0].end_row, 2);
        match &sheet.rows[2][1] {
            CellValue::Formula { formula, .. } => assert_eq!(formula, "=SUM(A2:A3)"),
            value => panic!("expected formula after projection refresh, got {value:?}"),
        }

        let (_, saved_bytes) = state
            .generate_file_bytes_for_target("projection-refresh.xlsx")
            .expect("save workbook");
        let saved = reader::xlsx::read_reader(Cursor::new(saved_bytes), true).expect("read saved");
        let saved_sheet = saved.sheet(0).expect("sheet");
        assert_eq!(saved_sheet.cell("B3").expect("B3").formula(), "SUM(A2:A3)");
        assert_eq!(
            saved_sheet.merge_cells()[0]
                .coordinate_start_row()
                .unwrap()
                .num(),
            2
        );
        assert!(
            saved_sheet
                .row_dimensions()
                .iter()
                .any(|row| row.row_num() == 2 && (row.height() - 84.0).abs() < 0.001)
        );
    }

    #[test]
    fn structure_edit_rejects_workbooks_with_unsupported_features() {
        let mut source = umya_spreadsheet::new_file();
        source
            .sheet_mut(0)
            .expect("sheet")
            .cell_mut("A1")
            .set_value_number(1);
        let mut defined_name = DefinedName::default();
        defined_name.set_name("Inputs");
        defined_name.set_address("Sheet1!$A$1");
        source.add_defined_names(defined_name);

        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "defined-name.xlsx".to_string(),
        )
        .expect("read source");

        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        let error = state
            .execute(EditorCommand::AddRow {
                sheet_index: 0,
                row_index: 0,
            })
            .expect_err("structure edit should be rejected");

        match error {
            AppError::UnsupportedWorkbookStructure(message) => {
                assert!(message.contains("defined names"), "message was {message}");
            }
            error => panic!("unexpected error: {error:?}"),
        }
        assert_eq!(
            state.file_data().sheets[0].rows[0][0].to_display_string(),
            "1"
        );
    }

    #[test]
    fn workbook_capabilities_disable_protected_workbook_edits() {
        let mut source = umya_spreadsheet::new_file();
        source
            .sheet_mut(0)
            .expect("sheet")
            .set_sheet_protection(SheetProtection::default());

        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "protected.xlsx".to_string(),
        )
        .expect("read source");

        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        let capabilities = state.capabilities();
        assert!(!capabilities.sheets[0].can_edit_cells);
        assert!(!capabilities.sheets[0].can_resize_rows_columns);
        assert!(!capabilities.sheets[0].can_insert_delete_rows);
        assert!(!capabilities.sheets[0].can_insert_delete_columns);
        assert!(capabilities.structure.can_insert_delete_sheets);
        assert!(
            capabilities.sheets[0]
                .blocked_row_structure_reasons
                .contains(&"sheet protection".to_string())
        );

        let revision = state.revision();
        let no_op = state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: String::new(),
            })
            .expect("no-op edit on protected sheet should not require workbook mutation support");
        assert!(no_op.operation.is_none());
        assert_eq!(state.revision(), revision);

        assert!(matches!(
            state.execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "blocked".to_string(),
            }),
            Err(AppError::UnsupportedWorkbookStructure(_))
        ));
        assert!(matches!(
            state.execute(EditorCommand::SetRowHeight {
                sheet_index: 0,
                row_index: 0,
                height: Some(80),
            }),
            Err(AppError::UnsupportedWorkbookStructure(_))
        ));
        assert!(matches!(
            state.execute(EditorCommand::AddRow {
                sheet_index: 0,
                row_index: 0,
            }),
            Err(AppError::UnsupportedWorkbookStructure(_))
        ));
    }

    #[test]
    fn sheets_allow_unprotected_sheets_to_remain_editable() {
        let mut source = umya_spreadsheet::new_file();
        source
            .sheet_mut(0)
            .expect("protected sheet")
            .set_sheet_protection(SheetProtection::default());
        source.new_sheet("Editable").expect("editable sheet");

        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "mixed-protection.xlsx".to_string(),
        )
        .expect("read source");

        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        let capabilities = state.capabilities();
        assert_eq!(capabilities.sheets.len(), 2);
        assert!(!capabilities.sheets[0].can_edit_cells);
        assert!(!capabilities.sheets[0].can_resize_rows_columns);
        assert!(!capabilities.sheets[0].can_insert_delete_rows);
        assert!(!capabilities.sheets[0].can_insert_delete_columns);
        assert!(capabilities.sheets[1].can_edit_cells);
        assert!(capabilities.sheets[1].can_resize_rows_columns);
        assert!(capabilities.sheets[1].can_insert_delete_rows);
        assert!(capabilities.sheets[1].can_insert_delete_columns);

        state
            .execute(EditorCommand::SetCell {
                sheet_index: 1,
                row: 0,
                col: 0,
                text: "editable".to_string(),
            })
            .expect("unprotected sheet remains editable");

        state
            .execute(EditorCommand::SetCells {
                changes: vec![
                    crate::domain::CellEditInput {
                        sheet_index: 0,
                        row: 0,
                        col: 0,
                        text: String::new(),
                    },
                    crate::domain::CellEditInput {
                        sheet_index: 1,
                        row: 0,
                        col: 1,
                        text: "batch editable".to_string(),
                    },
                ],
            })
            .expect("protected-sheet no-op should not block editable batch changes");
        assert_eq!(
            state.file_data().sheets[1].rows[0][1],
            CellValue::String("batch editable".to_string())
        );
        assert!(matches!(
            state.execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "blocked".to_string(),
            }),
            Err(AppError::UnsupportedWorkbookStructure(_))
        ));
    }

    #[test]
    fn row_height_and_column_width_participate_in_undo_redo() {
        let mut source = umya_spreadsheet::new_file();
        source
            .sheet_mut(0)
            .expect("sheet")
            .cell_mut("A1")
            .set_value_string("layout");

        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "layout.xlsx".to_string(),
        )
        .expect("read source");

        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        state
            .execute(EditorCommand::SetColumnWidth {
                sheet_index: 0,
                col_index: 0,
                width: Some(180),
            })
            .expect("set column width");
        state
            .execute(EditorCommand::SetRowHeight {
                sheet_index: 0,
                row_index: 0,
                height: Some(96),
            })
            .expect("set row height");

        assert_eq!(
            state.file_data().sheets[0]
                .column_widths
                .as_ref()
                .and_then(|widths| widths.get(&0)),
            Some(&180)
        );
        assert_eq!(
            state.file_data().sheets[0]
                .row_heights
                .as_ref()
                .and_then(|heights| heights.get(&0)),
            Some(&96)
        );

        state.undo().expect("undo row height").expect("undo result");
        assert!(state.file_data().sheets[0].row_heights.is_none());
        state.redo().expect("redo row height").expect("redo result");
        assert_eq!(
            state.file_data().sheets[0]
                .row_heights
                .as_ref()
                .and_then(|heights| heights.get(&0)),
            Some(&96)
        );

        let (_, saved_bytes) = state
            .generate_file_bytes_for_target("layout.xlsx")
            .expect("save from workbook");
        let saved = reader::xlsx::read_reader(Cursor::new(saved_bytes), true).expect("read saved");
        let sheet = saved.sheet(0).expect("saved sheet");

        assert!(
            sheet
                .column_dimensions()
                .iter()
                .any(|column| { column.col_num() == 1 && (column.width() - 25.0).abs() < 0.001 })
        );
        assert!(
            sheet
                .row_dimensions()
                .iter()
                .any(|row| row.row_num() == 1 && (row.height() - 72.0).abs() < 0.001)
        );
    }

    #[test]
    fn row_column_structure_undo_restores_persisted_layout() {
        let mut source = umya_spreadsheet::new_file();
        {
            let sheet = source.sheet_mut(0).expect("sheet");
            sheet.cell_mut("A1").set_value_string("a1");
            sheet.cell_mut("B1").set_value_string("b1");
            sheet.cell_mut("A2").set_value_string("a2");
            sheet.cell_mut("B2").set_value_string("b2");
            sheet.row_dimension_mut(1).set_height(84.0);
            sheet.column_dimension_by_number_mut(1).set_width(25.0);
        }

        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "layout-structure.xlsx".to_string(),
        )
        .expect("read source");

        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        assert_eq!(
            state.file_data().sheets[0]
                .row_heights
                .as_ref()
                .and_then(|heights| heights.get(&0)),
            Some(&112)
        );
        assert_eq!(
            state.file_data().sheets[0]
                .column_widths
                .as_ref()
                .and_then(|widths| widths.get(&0)),
            Some(&180)
        );

        state
            .execute(EditorCommand::DeleteRow {
                sheet_index: 0,
                row_index: 0,
            })
            .expect("delete row");
        assert!(
            state.file_data().sheets[0]
                .row_heights
                .as_ref()
                .is_none_or(|heights| !heights.contains_key(&0))
        );
        state.undo().expect("undo row delete").expect("undo result");
        assert_eq!(
            state.file_data().sheets[0]
                .row_heights
                .as_ref()
                .and_then(|heights| heights.get(&0)),
            Some(&112)
        );

        state
            .execute(EditorCommand::DeleteColumn {
                sheet_index: 0,
                col_index: 0,
            })
            .expect("delete column");
        assert!(
            state.file_data().sheets[0]
                .column_widths
                .as_ref()
                .is_none_or(|widths| !widths.contains_key(&0)),
            "column widths after delete: {:?}",
            state.file_data().sheets[0].column_widths
        );
        state
            .undo()
            .expect("undo column delete")
            .expect("undo result");
        assert_eq!(
            state.file_data().sheets[0]
                .column_widths
                .as_ref()
                .and_then(|widths| widths.get(&0)),
            Some(&180)
        );

        let (_, saved_bytes) = state
            .generate_file_bytes_for_target("layout-structure.xlsx")
            .expect("save from workbook");
        let saved = reader::xlsx::read_reader(Cursor::new(saved_bytes), true).expect("read saved");
        let sheet = saved.sheet(0).expect("saved sheet");
        assert!(
            sheet
                .row_dimensions()
                .iter()
                .any(|row| row.row_num() == 1 && (row.height() - 84.0).abs() < 0.001)
        );
        assert!(
            sheet
                .column_dimensions()
                .iter()
                .any(|column| { column.col_num() == 1 && (column.width() - 25.0).abs() < 0.001 })
        );
    }

    #[test]
    fn projection_only_structure_undo_preserves_other_sheets() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "generated.xlsx".to_string(),
                sheets: vec![
                    DocumentSheet {
                        name: "One".to_string(),
                        rows: vec![vec![CellValue::String("first".to_string())]],
                        ..Default::default()
                    },
                    DocumentSheet {
                        name: "Two".to_string(),
                        rows: vec![vec![CellValue::String("second".to_string())]],
                        ..Default::default()
                    },
                ],
            },
            None,
        );

        state
            .execute(EditorCommand::DeleteRow {
                sheet_index: 0,
                row_index: 0,
            })
            .expect("delete row");
        state.undo().expect("undo row delete").expect("undo result");

        assert_eq!(state.file_data().sheets.len(), 2);
        assert_eq!(
            state.file_data().sheets[0].rows[0][0],
            CellValue::String("first".to_string())
        );
        assert_eq!(
            state.file_data().sheets[1].rows[0][0],
            CellValue::String("second".to_string())
        );
    }

    #[test]
    fn set_cell_extends_sparse_projection_and_saved_workbook() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "sparse.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::String("A1".to_string())]],
                    ..Default::default()
                }],
            },
            None,
        );

        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 3,
                col: 4,
                text: "E4".to_string(),
            })
            .expect("set sparse cell");

        assert_eq!(
            state.file_data().sheets[0].rows[3][4],
            CellValue::String("E4".to_string())
        );

        let (_, saved_bytes) = state
            .generate_file_bytes_for_target("sparse.xlsx")
            .expect("save sparse workbook");
        let saved = reader::xlsx::read_reader(Cursor::new(saved_bytes), true).expect("read saved");
        assert_eq!(
            saved
                .sheet(0)
                .expect("sheet")
                .cell("E4")
                .expect("E4")
                .value(),
            "E4"
        );
    }

    #[test]
    fn set_cell_undo_restores_sparse_projection_shape() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "sparse.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::String("A1".to_string())]],
                    ..Default::default()
                }],
            },
            None,
        );

        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 3,
                col: 4,
                text: "E4".to_string(),
            })
            .expect("set sparse cell");
        state.undo().expect("undo").expect("undo result");

        assert_eq!(state.file_data().sheets[0].rows.len(), 1);
        assert_eq!(state.file_data().sheets[0].rows[0].len(), 1);
        assert_eq!(
            state.file_data().sheets[0].rows[0][0],
            CellValue::String("A1".to_string())
        );

        let (_, saved_bytes) = state
            .generate_file_bytes_for_target("sparse.xlsx")
            .expect("save sparse workbook");
        let saved = reader::xlsx::read_reader(Cursor::new(saved_bytes), true).expect("read saved");
        assert!(saved.sheet(0).expect("sheet").cell("E4").is_none());
    }

    #[test]
    fn structure_redo_restores_sparse_projection_shape() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "sparse-structure.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::String("A1".to_string())], vec![]],
                    column_widths: Some(std::collections::HashMap::from([(3, 120)])),
                    row_heights: Some(std::collections::HashMap::from([(3, 72)])),
                    ..Default::default()
                }],
            },
            None,
        );

        state
            .execute(EditorCommand::AddColumn {
                sheet_index: 0,
                col_index: 3,
            })
            .expect("add sparse column");
        assert_eq!(
            state.file_data().sheets[0]
                .rows
                .iter()
                .map(Vec::len)
                .collect::<Vec<_>>(),
            vec![4, 4, 4, 4]
        );
        state.undo().expect("undo column add").expect("undo result");
        assert_eq!(
            state.file_data().sheets[0]
                .rows
                .iter()
                .map(Vec::len)
                .collect::<Vec<_>>(),
            vec![1, 0]
        );
        state.redo().expect("redo column add").expect("redo result");
        assert_eq!(
            state.file_data().sheets[0]
                .rows
                .iter()
                .map(Vec::len)
                .collect::<Vec<_>>(),
            vec![4, 4, 4, 4]
        );

        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "sparse-structure.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::String("A1".to_string())]],
                    row_heights: Some(std::collections::HashMap::from([(3, 72)])),
                    ..Default::default()
                }],
            },
            None,
        );

        state
            .execute(EditorCommand::AddRow {
                sheet_index: 0,
                row_index: 3,
            })
            .expect("add sparse row");
        assert_eq!(state.file_data().sheets[0].rows.len(), 4);
        state.undo().expect("undo row add").expect("undo result");
        assert_eq!(state.file_data().sheets[0].rows.len(), 1);
        state.redo().expect("redo row add").expect("redo result");
        assert_eq!(state.file_data().sheets[0].rows.len(), 4);
        assert_eq!(state.file_data().sheets[0].rows[3].len(), 1);
    }

    #[test]
    fn undo_sparse_cell_edit_preserves_style_only_far_cells() {
        let mut source = umya_spreadsheet::new_file();
        {
            let sheet = source.sheet_mut(0).expect("sheet");
            sheet.cell_mut("A1").set_value_string("value");
            sheet
                .cell_mut("Z1000")
                .style_mut()
                .font_mut()
                .set_bold(true);
        }

        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "style-only-far-cell.xlsx".to_string(),
        )
        .expect("read source");
        assert_eq!(parsed.file_data.sheets[0].rows.len(), 1);
        assert_eq!(parsed.file_data.sheets[0].rows[0].len(), 1);
        assert!(
            parsed.file_data.sheets[0]
                .rich
                .cell_styles
                .contains_key("Z1000")
        );

        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 3,
                col: 4,
                text: "E4".to_string(),
            })
            .expect("set sparse cell");
        state.undo().expect("undo").expect("undo result");

        let (_, saved_bytes) = state
            .generate_file_bytes_for_target("style-only-far-cell.xlsx")
            .expect("save workbook");
        let saved = reader::xlsx::read_reader(Cursor::new(saved_bytes), true).expect("read saved");
        let far_cell = saved
            .sheet(0)
            .expect("sheet")
            .cell("Z1000")
            .expect("style-only far cell");
        assert!(far_cell.style().font().is_some_and(|font| font.bold()));
        assert!(saved.sheet(0).expect("sheet").cell("E4").is_none());
    }

    #[test]
    fn undo_redo_restores_workbook_snapshot_styles() {
        let mut source = umya_spreadsheet::new_file();
        {
            let sheet = source.sheet_mut(0).expect("sheet");
            sheet.cell_mut("A1").set_value_string("styled");
            sheet
                .cell_mut("A1")
                .style_mut()
                .set_background_color(Color::COLOR_RED_STR);
        }

        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "styled-undo.xlsx".to_string(),
        )
        .expect("read source");

        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "changed".to_string(),
            })
            .expect("set cell");
        state.undo().expect("undo").expect("undo result");
        state.redo().expect("redo").expect("redo result");

        let (_, saved_bytes) = state
            .generate_file_bytes_for_target("styled-undo.xlsx")
            .expect("save workbook");
        let saved = reader::xlsx::read_reader(Cursor::new(saved_bytes), true).expect("read saved");
        let cell = saved.sheet(0).expect("sheet").cell("A1").expect("A1");

        assert_eq!(cell.value(), "changed");
        assert_eq!(
            cell.style()
                .background_color()
                .map(|color| color.argb_str()),
            Some(Color::COLOR_RED_STR.to_string())
        );
    }

    #[test]
    fn structure_edits_adjust_and_save_merge_ranges() {
        let mut source = umya_spreadsheet::new_file();
        {
            let sheet = source.sheet_mut(0).expect("sheet");
            sheet.cell_mut("A1").set_value_string("merged");
            sheet.add_merge_cells("A1:C3");
        }

        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "merged-structure.xlsx".to_string(),
        )
        .expect("read source");

        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        state
            .execute(EditorCommand::DeleteRow {
                sheet_index: 0,
                row_index: 0,
            })
            .expect("delete first row");
        let merges = &state.file_data().sheets[0].merges;
        assert!(merges.is_empty());

        let (_, saved_bytes) = state
            .generate_file_bytes_for_target("merged-structure.xlsx")
            .expect("save workbook");
        let saved = reader::xlsx::read_reader(Cursor::new(saved_bytes), true).expect("read saved");
        let saved_sheet = saved.sheet(0).expect("sheet");
        let saved_merges = saved_sheet.merge_cells();
        assert!(saved_merges.is_empty());
    }

    #[test]
    fn csv_document_can_export_xlsx_from_projection() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: "input.csv".to_string(),
                file_name: "input.csv".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::String("csv".to_string())]],
                    ..Default::default()
                }],
            },
            None,
        );

        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 1,
                text: "xlsx".to_string(),
            })
            .expect("edit csv projection");

        let (_, saved_bytes) = state
            .generate_file_bytes_for_target("export.xlsx")
            .expect("export projection as xlsx");
        let saved = reader::xlsx::read_reader(Cursor::new(saved_bytes), true).expect("read saved");
        let sheet = saved.sheet(0).expect("sheet");
        assert_eq!(sheet.cell("A1").expect("A1").value(), "csv");
        assert_eq!(sheet.cell("B1").expect("B1").value(), "xlsx");
    }

    #[test]
    fn csv_saved_as_xlsx_rebinds_workbook_capabilities() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: "input.csv".to_string(),
                file_name: "input.csv".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::String("csv".to_string())]],
                    ..Default::default()
                }],
            },
            None,
        );
        assert!(!state.capabilities().sheets[0].can_resize_rows_columns);
        assert!(!state.capabilities().structure.can_insert_delete_sheets);

        let (saved_name, saved_bytes) = state
            .generate_file_bytes_for_target("converted.xlsx")
            .expect("save projection as xlsx");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            saved_bytes,
            "converted.xlsx".to_string(),
            saved_name,
        )
        .expect("read saved xlsx");

        let retired = state
            .rebind_saved_document(parsed.file_data, parsed.workbook, true)
            .expect("rebind saved document");
        assert_eq!(
            retired
                ._document
                .as_ref()
                .map(|document| document.projection().file_name.as_str()),
            Some("input.csv")
        );
        assert!(retired._history.is_some());
        drop(retired);
        state.mark_saved();

        assert!(state.capabilities().sheets[0].can_resize_rows_columns);
        assert!(state.capabilities().structure.can_insert_delete_sheets);
        assert!(!state.is_dirty());
    }

    #[test]
    fn csv_capabilities_disable_unpersisted_features() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: "input.csv".to_string(),
                file_name: "input.csv".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::String("csv".to_string())]],
                    ..Default::default()
                }],
            },
            None,
        );

        let capabilities = state.capabilities();
        assert!(capabilities.sheets[0].can_edit_cells);
        assert!(capabilities.sheets[0].can_insert_delete_rows);
        assert!(capabilities.sheets[0].can_insert_delete_columns);
        assert!(!capabilities.sheets[0].can_resize_rows_columns);
        assert!(!capabilities.structure.can_insert_delete_sheets);

        assert!(
            state
                .execute(EditorCommand::SetRowHeight {
                    sheet_index: 0,
                    row_index: 0,
                    height: Some(96),
                })
                .is_err()
        );
        assert!(
            state
                .execute(EditorCommand::AddSheet { name: None })
                .is_err()
        );
        state
            .execute(EditorCommand::AddRow {
                sheet_index: 0,
                row_index: 1,
            })
            .expect("CSV row insertion is persisted as values");
    }

    #[test]
    fn batched_cell_edits_share_one_history_entry_and_patch_workbook() {
        let mut source = umya_spreadsheet::new_file();
        {
            let sheet = source.sheet_mut(0).expect("sheet");
            sheet.cell_mut("A1").set_value_string("a");
            sheet.cell_mut("B1").set_value_string("b");
            sheet.cell_mut("C1").set_formula("A1&B1");
            sheet.cell_mut("C1").set_formula_result_string("ab");
        }

        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "batch.xlsx".to_string(),
        )
        .expect("read source");

        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        state
            .execute(EditorCommand::SetCells {
                changes: vec![
                    crate::domain::CellEditInput {
                        sheet_index: 0,
                        row: 0,
                        col: 0,
                        text: "x".to_string(),
                    },
                    crate::domain::CellEditInput {
                        sheet_index: 0,
                        row: 0,
                        col: 1,
                        text: "y".to_string(),
                    },
                ],
            })
            .expect("batch edit");

        assert!(state.can_undo());
        assert_eq!(
            state.file_data().sheets[0].rows[0][0],
            CellValue::String("x".to_string())
        );
        assert_eq!(
            state.file_data().sheets[0].rows[0][1],
            CellValue::String("y".to_string())
        );
        state.undo().expect("undo").expect("undo result");
        assert_eq!(
            state.file_data().sheets[0].rows[0][0],
            CellValue::String("a".to_string())
        );
        assert_eq!(
            state.file_data().sheets[0].rows[0][1],
            CellValue::String("b".to_string())
        );
        state.redo().expect("redo").expect("redo result");

        let (_, saved_bytes) = state
            .generate_file_bytes_for_target("batch.xlsx")
            .expect("save");
        let saved = reader::xlsx::read_reader(Cursor::new(saved_bytes), true).expect("read saved");
        let sheet = saved.sheet(0).expect("sheet");
        assert_eq!(sheet.cell("A1").expect("A1").value(), "x");
        assert_eq!(sheet.cell("B1").expect("B1").value(), "y");
        assert!(sheet.cell("C1").expect("C1").cell_value().is_formula());
    }

    #[test]
    fn editing_formula_cell_to_plain_value_clears_saved_formula() {
        let mut source = umya_spreadsheet::new_file();
        {
            let sheet = source.sheet_mut(0).expect("sheet");
            sheet.cell_mut("A1").set_value_number(1);
            sheet.cell_mut("B1").set_formula("A1+1");
            sheet.cell_mut("B1").set_formula_result_number(2.0);
        }

        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "formula-to-value.xlsx".to_string(),
        )
        .expect("read source");

        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 1,
                text: "plain".to_string(),
            })
            .expect("edit formula to plain value");

        let (_, saved_bytes) = state
            .generate_file_bytes_for_target("formula-to-value.xlsx")
            .expect("save");
        let saved = reader::xlsx::read_reader(Cursor::new(saved_bytes), true).expect("read saved");
        let cell = saved.sheet(0).expect("sheet").cell("B1").expect("B1");

        assert_eq!(cell.value(), "plain");
        assert!(
            !cell.cell_value().is_formula(),
            "saved B1 should be a plain value, formula was {:?}",
            cell.formula()
        );
    }

    #[test]
    fn batched_invalid_formula_returns_error_cell_and_keeps_dependencies_live() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "batch-formula.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![
                        CellValue::Number(CellNumber::from(1)),
                        CellValue::formula("=A1+1", CellValue::Null),
                        CellValue::formula("=A1+2", CellValue::Null),
                    ]],
                    ..Default::default()
                }],
            },
            None,
        );

        let result = state
            .execute(EditorCommand::SetCells {
                changes: vec![
                    crate::domain::CellEditInput {
                        sheet_index: 0,
                        row: 0,
                        col: 1,
                        text: "=SUM(".to_string(),
                    },
                    crate::domain::CellEditInput {
                        sheet_index: 0,
                        row: 0,
                        col: 0,
                        text: "10".to_string(),
                    },
                ],
            })
            .expect("batch formula edit");

        assert!(result.cell_changes.iter().any(|change| {
            change.sheet_index == 0
                && change.row == 0
                && change.col == 1
                && matches!(&change.value, CellValue::Formula { error: Some(_), .. })
        }));
        assert!(matches!(
            &state.file_data().sheets[0].rows[0][1],
            CellValue::Formula { error: Some(_), .. }
        ));
        assert_eq!(
            state.file_data().sheets[0].rows[0][2].to_display_string(),
            "12.0"
        );

        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "20".to_string(),
            })
            .expect("dependency edit");
        assert_eq!(
            state.file_data().sheets[0].rows[0][2].to_display_string(),
            "22.0"
        );
    }

    #[test]
    fn delete_sheet_invalidates_external_formula_references() {
        let mut source = umya_spreadsheet::new_file();
        source.new_sheet("Calc").expect("calc sheet");
        {
            let inputs = source.sheet_mut(0).expect("inputs");
            inputs.set_name("Inputs");
            inputs.cell_mut("A1").set_value_number(1);
        }
        {
            let calc = source.sheet_mut(1).expect("calc");
            calc.cell_mut("A1").set_formula("Inputs!A1+1");
            calc.cell_mut("A1").set_formula_result_number(2.0);
        }

        let mut bytes = Vec::new();
        writer::xlsx::write_writer(&source, &mut bytes).expect("write source");
        let parsed = read_file_with_workbook_from_bytes(
            "xlsx",
            bytes,
            String::new(),
            "delete-sheet-ref.xlsx".to_string(),
        )
        .expect("read source");

        let mut state = EditorState::with_workbook(parsed.file_data, parsed.workbook);
        state
            .execute(EditorCommand::DeleteSheet { sheet_index: 0 })
            .expect("delete referenced sheet");

        let (_, saved_bytes) = state
            .generate_file_bytes_for_target("delete-sheet-ref.xlsx")
            .expect("save workbook");
        let saved = reader::xlsx::read_reader(Cursor::new(saved_bytes), true).expect("read saved");
        let formula = saved
            .sheet(0)
            .expect("calc")
            .cell("A1")
            .expect("A1")
            .formula()
            .to_string();
        assert!(formula.contains("#REF!"), "formula was {formula}");
    }

    #[test]
    fn history_is_bounded() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "history.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::Null]],
                    ..Default::default()
                }],
            },
            None,
        );

        for index in 0..105 {
            state
                .execute(EditorCommand::SetCell {
                    sheet_index: 0,
                    row: 0,
                    col: 0,
                    text: index.to_string(),
                })
                .expect("edit");
        }

        assert_eq!(state.history.undo_len(), MAX_HISTORY_ENTRIES);
        assert!(state.history.undo_estimated_bytes() <= MAX_HISTORY_BYTES);
        assert!(state.can_undo());
    }

    #[test]
    fn new_edit_returns_cleared_redo_history_for_external_release() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "retired-history.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::Null]],
                    ..Default::default()
                }],
            },
            None,
        );
        let first = state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "first".to_string(),
            })
            .expect("first edit");
        assert_eq!(first.retired.retired_history_entry_count(), 0);
        drop(first);
        let undo = state.undo().expect("undo").expect("undo result");
        assert_eq!(undo.retired.retired_history_entry_count(), 0);
        drop(undo);

        let second = state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "second".to_string(),
            })
            .expect("second edit");

        assert_eq!(second.retired.retired_history_entry_count(), 1);
        assert!(!state.can_redo());
    }

    #[test]
    fn history_is_bounded_by_estimated_bytes() {
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "history-memory.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::Null]],
                    ..Default::default()
                }],
            },
            None,
        );
        let large_text = "x".repeat(2 * 1024 * 1024);

        for index in 0..40 {
            state
                .execute(EditorCommand::SetCell {
                    sheet_index: 0,
                    row: 0,
                    col: 0,
                    text: format!("{large_text}{index}"),
                })
                .expect("edit");
        }

        assert!(state.history.undo_len() < 40);
        assert!(state.history.undo_estimated_bytes() <= MAX_HISTORY_BYTES);
        assert!(state.can_undo());
    }

    #[test]
    fn no_op_cell_edit_bypasses_memento_budget() {
        let huge_text = "x".repeat(MAX_SINGLE_HISTORY_ENTRY_BYTES + 1024);
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "huge-no-op.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Huge".to_string(),
                    rows: vec![vec![CellValue::String(huge_text.clone())]],
                    ..Default::default()
                }],
            },
            None,
        );

        let revision = state.revision();
        let result = state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: huge_text,
            })
            .expect("unchanged cell should not allocate rollback history");

        assert!(result.operation.is_none());
        assert_eq!(state.revision(), revision);
        assert!(!state.can_undo());
    }

    #[test]
    fn oversized_structure_memento_is_rejected_before_history_capture() {
        let huge_text = "x".repeat(MAX_SINGLE_HISTORY_ENTRY_BYTES + 1024);
        let mut state = EditorState::with_workbook(
            DocumentData {
                path: String::new(),
                file_name: "huge-structure.xlsx".to_string(),
                sheets: vec![
                    DocumentSheet {
                        name: "Huge".to_string(),
                        rows: vec![vec![CellValue::String(huge_text)]],
                        ..Default::default()
                    },
                    DocumentSheet {
                        name: "Other".to_string(),
                        rows: vec![vec![CellValue::Null]],
                        ..Default::default()
                    },
                ],
            },
            None,
        );

        let error = state
            .execute(EditorCommand::DeleteSheet { sheet_index: 0 })
            .expect_err("oversized structure operation should be rejected");

        assert!(error.to_string().contains("too large for safe undo"));
        assert!(!state.can_undo());
        assert_eq!(state.file_data().sheets.len(), 2);
    }
}
