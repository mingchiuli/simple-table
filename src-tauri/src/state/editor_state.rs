use crate::error::AppError;
use crate::io::document_memento::MementoSide;
use crate::io::document_model::{DocumentRestoreResult, SpreadsheetDocument};
use crate::io::document_save::SpreadsheetDocumentSaveSnapshot;
use crate::ops::EditorCommand;
#[cfg(test)]
use crate::state::content_hash::ContentHash;
use crate::state::dirty_tracker::DirtyTracker;
use crate::state::editor_session::EditorSession;
use crate::state::history_store::{HistoryEntry, HistoryStore, MAX_SINGLE_HISTORY_ENTRY_BYTES};
#[cfg(test)]
use crate::state::history_store::{MAX_HISTORY_BYTES, MAX_HISTORY_ENTRIES};
use crate::state::search_index::{
    SearchCellText, SearchIndexStamp, SearchSheetIndex, SearchWriterHandle,
};
use crate::state::search_session::SearchSession;
use crate::state::state::HistoryStatus;
use crate::types::{
    AppliedOperationResult, FileData, FormulaStatus, SheetCellChange, WorkbookCapabilities,
};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use umya_spreadsheet::Workbook;

static NEXT_SAVE_COMMIT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct ExecutedOperation {
    pub operation: Option<AppliedOperationResult>,
    pub cell_changes: Vec<SheetCellChange>,
    pub restore: Option<DocumentRestoreResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveCommitLease {
    document_id: u64,
    revision: u64,
    token: u64,
}

/// 编辑器状态管理器
pub struct EditorState {
    session: EditorSession,
    document: SpreadsheetDocument,
    history: HistoryStore,
    dirty: DirtyTracker,
    search: SearchSession,
    save_commit: Option<SaveCommitLease>,
}

impl EditorState {
    pub fn with_workbook(file_data: FileData, workbook: Option<Workbook>) -> Self {
        let document = SpreadsheetDocument::new(file_data, workbook);
        let content_hash = document.content_hash();
        Self {
            session: EditorSession::new(),
            document,
            history: HistoryStore::default(),
            dirty: DirtyTracker::new(content_hash),
            search: SearchSession::default(),
            save_commit: None,
        }
    }

    pub fn file_data(&self) -> &FileData {
        self.document.projection()
    }

    pub fn update_identity(&mut self, path: String, file_name: String) {
        if self.has_save_commit_in_progress() {
            return;
        }
        self.document.update_identity(path, file_name);
    }

    pub fn rebind_saved_document(
        &mut self,
        file_data: FileData,
        workbook: Option<Workbook>,
        clear_history: bool,
    ) {
        self.document = SpreadsheetDocument::new(file_data, workbook);
        if clear_history {
            self.history.clear_all();
        }
        self.bump_revision();
        self.refresh_content_hash();
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
        if self.document_id() != document_id || self.revision() != revision {
            return Err(AppError::DocumentStateInvalid(
                "document changed while save was in progress; please save again".to_string(),
            ));
        }

        let lease = SaveCommitLease {
            document_id,
            revision,
            token: NEXT_SAVE_COMMIT_ID.fetch_add(1, Ordering::Relaxed),
        };
        self.save_commit = Some(lease);
        Ok(lease)
    }

    pub fn abort_save_commit(&mut self, lease: SaveCommitLease) {
        if self.save_commit == Some(lease) {
            self.save_commit = None;
        }
    }

    pub fn finish_save_commit(
        &mut self,
        lease: SaveCommitLease,
        file_data: FileData,
        workbook: Option<Workbook>,
        clear_history: bool,
    ) -> Result<(), AppError> {
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
        self.rebind_saved_document(file_data, workbook, clear_history);
        self.mark_saved();
        self.mark_search_index_stale();
        Ok(())
    }

    #[cfg(test)]
    pub fn can_finish_save_without_reparse(&self, target_extension: &str) -> bool {
        target_extension.eq_ignore_ascii_case("xlsx") && self.document.is_excel_backed()
    }

    pub fn save_snapshot_for_target(
        &self,
        target_path_or_name: &str,
    ) -> Result<SpreadsheetDocumentSaveSnapshot, AppError> {
        self.document.save_snapshot_for_target(target_path_or_name)
    }

    pub fn finish_save_commit_without_reparse(
        &mut self,
        lease: SaveCommitLease,
        path: String,
        file_name: String,
        clear_history: bool,
    ) -> Result<(), AppError> {
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
        self.document.update_identity(path, file_name);
        if clear_history {
            self.history.clear_all();
        }
        self.bump_revision();
        self.refresh_content_hash();
        self.mark_saved();
        Ok(())
    }

    pub fn document_id(&self) -> u64 {
        self.session.document_id()
    }

    pub fn revision(&self) -> u64 {
        self.session.revision()
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
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

    pub fn transaction_failure(&self) -> Option<&str> {
        self.document.transaction_failure()
    }

    pub fn search_sheet_index_stamp(&self, sheet_index: usize) -> SearchIndexStamp {
        self.search.sheet_stamp(self.document_id(), sheet_index)
    }

    pub fn install_search_index(
        &mut self,
        sheet_index: usize,
        stamp: SearchIndexStamp,
        index: Option<SearchSheetIndex>,
    ) {
        self.search.install_sheet_index(
            self.document_id(),
            sheet_index,
            self.file_data().sheets.len(),
            stamp,
            index,
        );
    }

    pub fn mark_search_index_stale(&mut self) -> SearchIndexStamp {
        self.search.mark_all_stale(self.document_id())
    }

    pub fn mark_search_sheets_stale(&mut self, sheet_indexes: impl IntoIterator<Item = usize>) {
        self.search.mark_sheets_stale(sheet_indexes);
    }

    pub fn mark_search_sheet_fresh(&mut self, sheet_index: usize, stamp: SearchIndexStamp) {
        self.search
            .mark_sheet_fresh(self.document_id(), sheet_index, stamp);
    }

    pub fn search_writer_handle(
        &self,
        sheet_index: usize,
        stamp: SearchIndexStamp,
    ) -> Option<SearchWriterHandle> {
        self.search
            .writer_handle(self.document_id(), sheet_index, stamp)
    }

    pub fn indexed_search_sheet(
        &self,
        sheet_index: usize,
        query: &str,
        limit: usize,
    ) -> Option<Vec<SearchCellText>> {
        self.search.indexed_search_sheet(sheet_index, query, limit)
    }

    pub fn sheet_name(&self, sheet_index: usize) -> Option<String> {
        self.file_data()
            .sheets
            .get(sheet_index)
            .map(|sheet| sheet.name.clone())
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty.is_dirty()
    }

    pub fn mark_saved(&mut self) {
        self.dirty.mark_saved(self.document.content_hash());
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
        self.ensure_not_saving()?;
        let operation = command.resolve(self.file_data())?;
        self.ensure_operation_supported(&operation)?;
        self.ensure_memento_budget(&operation)?;
        let should_mark_search_stale = operation.impact().requires_search_rebuild();
        let before = self.document.capture_memento_side(&operation);
        if operation.impact().is_noop() {
            self.refresh_content_hash();
            return Ok(ExecutedOperation {
                operation: None,
                cell_changes: Vec::new(),
                restore: None,
            });
        }

        let result = self.document.execute_operation(&operation, &before)?;
        let stale_sheets = operation.search_stale_sheets(&result.cell_changes);
        let operation_result = result.operation;
        let cell_changes = result.cell_changes;

        if before.estimated_bytes() > MAX_SINGLE_HISTORY_ENTRY_BYTES {
            self.history.clear_all();
        } else {
            let after = self.document.capture_memento_side(&operation);
            let memento = SpreadsheetDocument::create_memento(before, after);
            let entry = HistoryEntry::new(memento);
            self.history.record(entry);
        }

        self.bump_revision();
        if should_mark_search_stale {
            self.mark_search_index_stale();
        } else {
            self.mark_search_sheets_stale(stale_sheets);
        }
        self.refresh_content_hash();
        Ok(ExecutedOperation {
            operation: Some(operation_result),
            cell_changes,
            restore: None,
        })
    }

    /// 撤销上一个操作
    pub fn undo(&mut self) -> Result<Option<ExecutedOperation>, AppError> {
        self.ensure_not_saving()?;
        if let Some(entry) = self.history.pop_undo() {
            let restore = self
                .document
                .restore_memento(&entry.memento, MementoSide::Before)?;
            self.history.push_redo(entry);
            self.bump_revision();
            self.mark_search_index_stale();
            self.refresh_content_hash();
            Ok(Some(ExecutedOperation {
                operation: None,
                cell_changes: Vec::new(),
                restore: Some(restore),
            }))
        } else {
            Ok(None)
        }
    }

    /// 重做上一个被撤销的操作
    pub fn redo(&mut self) -> Result<Option<ExecutedOperation>, AppError> {
        self.ensure_not_saving()?;
        if let Some(entry) = self.history.pop_redo() {
            let restore = self
                .document
                .restore_memento(&entry.memento, MementoSide::After)?;
            self.history.push_undo(entry);
            self.bump_revision();
            self.mark_search_index_stale();
            self.refresh_content_hash();
            Ok(Some(ExecutedOperation {
                operation: None,
                cell_changes: Vec::new(),
                restore: Some(restore),
            }))
        } else {
            Ok(None)
        }
    }

    fn refresh_content_hash(&mut self) {
        self.dirty.refresh(self.document.content_hash());
    }

    fn bump_revision(&mut self) {
        self.session.bump_revision();
    }

    fn ensure_operation_supported(
        &mut self,
        operation: &crate::ops::AppliedOperation,
    ) -> Result<(), AppError> {
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

    fn ensure_not_saving(&self) -> Result<(), AppError> {
        if self.save_commit.is_some() {
            return Err(AppError::DocumentStateInvalid(
                "save is already in progress".to_string(),
            ));
        }
        Ok(())
    }

    fn ensure_memento_budget(
        &mut self,
        operation: &crate::ops::AppliedOperation,
    ) -> Result<(), AppError> {
        let estimated_bytes = self.document.estimate_memento_side_bytes(operation);
        if estimated_bytes > MAX_SINGLE_HISTORY_ENTRY_BYTES {
            return Err(AppError::DocumentStateInvalid(format!(
                "operation is too large for safe undo/rollback history (estimated {estimated_bytes} bytes, limit {MAX_SINGLE_HISTORY_ENTRY_BYTES} bytes)"
            )));
        }
        Ok(())
    }
}

trait SearchInvalidation {
    fn search_stale_sheets(&self, formula_changes: &[SheetCellChange]) -> Vec<usize>;
}

impl SearchInvalidation for crate::ops::AppliedOperation {
    fn search_stale_sheets(&self, formula_changes: &[SheetCellChange]) -> Vec<usize> {
        let mut sheets = HashSet::new();
        match self {
            crate::ops::AppliedOperation::SetCell { sheet_index, .. } => {
                sheets.insert(*sheet_index);
            }
            crate::ops::AppliedOperation::SetCells { changes } => {
                for change in changes {
                    sheets.insert(change.sheet_index);
                }
            }
            _ => {}
        }
        for change in formula_changes {
            sheets.insert(change.sheet_index);
        }
        sheets.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::io::codec::reader::read_file_with_workbook_from_bytes;
    use crate::ops::EditorCommand;
    use crate::types::CellValue;
    use serde_json::Value;
    use umya_spreadsheet::{Color, DefinedName, SheetProtection, reader, writer};

    #[test]
    fn updating_file_identity_does_not_mark_content_dirty() {
        let mut state = EditorState::with_workbook(
            FileData {
                path: "/tmp/source.xlsx".to_string(),
                file_name: "source.xlsx".to_string(),
                sheets: vec![crate::types::SheetData {
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
    fn save_commit_lease_blocks_mutations_until_released() {
        let mut state = EditorState::with_workbook(
            FileData {
                path: "/tmp/source.xlsx".to_string(),
                file_name: "source.xlsx".to_string(),
                sheets: vec![crate::types::SheetData {
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

        assert!(state.can_finish_save_without_reparse("xlsx"));
        assert!(!state.can_finish_save_without_reparse("csv"));

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
            sheet.cell_mut("B1").set_formula("Inputs!A1");
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
            CellValue::Formula { formula, .. } => assert_eq!(formula, "=Inputs!A2"),
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
            "Inputs!A2"
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
            other.cell_mut("A1").set_formula("Inputs!A2");
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
            CellValue::Formula { formula, .. } => assert_eq!(formula, "=Inputs!A3"),
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
            "Inputs!A3"
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
            FileData {
                path: String::new(),
                file_name: "generated.xlsx".to_string(),
                sheets: vec![
                    crate::types::SheetData {
                        name: "One".to_string(),
                        rows: vec![vec![CellValue::String("first".to_string())]],
                        ..Default::default()
                    },
                    crate::types::SheetData {
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
            FileData {
                path: String::new(),
                file_name: "sparse.xlsx".to_string(),
                sheets: vec![crate::types::SheetData {
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
            FileData {
                path: String::new(),
                file_name: "sparse.xlsx".to_string(),
                sheets: vec![crate::types::SheetData {
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
            FileData {
                path: "input.csv".to_string(),
                file_name: "input.csv".to_string(),
                sheets: vec![crate::types::SheetData {
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
            FileData {
                path: "input.csv".to_string(),
                file_name: "input.csv".to_string(),
                sheets: vec![crate::types::SheetData {
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

        state.rebind_saved_document(parsed.file_data, parsed.workbook, true);
        state.mark_saved();

        assert!(state.capabilities().sheets[0].can_resize_rows_columns);
        assert!(state.capabilities().structure.can_insert_delete_sheets);
        assert!(!state.is_dirty());
    }

    #[test]
    fn csv_capabilities_disable_unpersisted_features() {
        let mut state = EditorState::with_workbook(
            FileData {
                path: "input.csv".to_string(),
                file_name: "input.csv".to_string(),
                sheets: vec![crate::types::SheetData {
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
                    crate::types::SetCellRequest {
                        sheet_index: 0,
                        row: 0,
                        col: 0,
                        text: "x".to_string(),
                    },
                    crate::types::SetCellRequest {
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
    fn batched_invalid_formula_returns_error_cell_and_keeps_dependencies_live() {
        let mut state = EditorState::with_workbook(
            FileData {
                path: String::new(),
                file_name: "batch-formula.xlsx".to_string(),
                sheets: vec![crate::types::SheetData {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![
                        CellValue::Number(Value::from(1)),
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
                    crate::types::SetCellRequest {
                        sheet_index: 0,
                        row: 0,
                        col: 1,
                        text: "=SUM(".to_string(),
                    },
                    crate::types::SetCellRequest {
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
            FileData {
                path: String::new(),
                file_name: "history.xlsx".to_string(),
                sheets: vec![crate::types::SheetData {
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
    fn history_is_bounded_by_estimated_bytes() {
        let mut state = EditorState::with_workbook(
            FileData {
                path: String::new(),
                file_name: "history-memory.xlsx".to_string(),
                sheets: vec![crate::types::SheetData {
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
    fn oversized_structure_memento_is_rejected_before_history_capture() {
        let huge_text = "x".repeat(MAX_SINGLE_HISTORY_ENTRY_BYTES + 1024);
        let mut state = EditorState::with_workbook(
            FileData {
                path: String::new(),
                file_name: "huge-structure.xlsx".to_string(),
                sheets: vec![
                    crate::types::SheetData {
                        name: "Huge".to_string(),
                        rows: vec![vec![CellValue::String(huge_text)]],
                        ..Default::default()
                    },
                    crate::types::SheetData {
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
