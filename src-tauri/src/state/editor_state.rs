use crate::error::AppError;
use crate::io::document_model::{
    DocumentMemento, DocumentRestoreResult, MementoSide, SpreadsheetDocument,
};
use crate::ops::EditorCommand;
use crate::state::content_hash::ContentHash;
use crate::state::search_index::{
    SearchIndexStamp, SearchIndexStore, SearchSheetIndex, SearchWriterHandle,
};
use crate::types::{
    AppliedOperationResult, CellPosition, FileData, FormulaStatus, SheetCellChange,
    WorkbookCapabilities,
};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use umya_spreadsheet::Workbook;

static NEXT_DOCUMENT_ID: AtomicU64 = AtomicU64::new(1);
const MAX_HISTORY_ENTRIES: usize = 100;
const MAX_HISTORY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ExecutedOperation {
    pub operation: Option<AppliedOperationResult>,
    pub cell_changes: Vec<SheetCellChange>,
    pub restore: Option<DocumentRestoreResult>,
}

#[derive(Debug, Clone)]
pub struct SearchCellSnapshot {
    pub row: usize,
    pub col: usize,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct SearchSheetSnapshot {
    pub sheet_index: usize,
    pub sheet_name: String,
    pub cells: Vec<SearchCellSnapshot>,
}

struct HistoryEntry {
    memento: DocumentMemento,
    operation: AppliedOperationResult,
    estimated_bytes: usize,
}

impl HistoryEntry {
    fn new(memento: DocumentMemento, operation: AppliedOperationResult) -> Self {
        let estimated_bytes = memento.estimated_bytes().max(1);
        Self {
            memento,
            operation,
            estimated_bytes,
        }
    }
}

/// 编辑器状态管理器
pub struct EditorState {
    document_id: u64,
    revision: u64,
    document: SpreadsheetDocument,
    history: Vec<HistoryEntry>,
    redo_stack: Vec<HistoryEntry>,
    history_estimated_bytes: usize,
    redo_estimated_bytes: usize,
    pub can_undo: bool,
    pub can_redo: bool,
    pub current_content_hash: ContentHash,
    pub saved_content_hash: ContentHash,
    search_index: SearchIndexStore,
}

impl EditorState {
    pub fn with_workbook(file_data: FileData, workbook: Option<Workbook>) -> Self {
        let document = SpreadsheetDocument::new(file_data, workbook);
        let content_hash = document.content_hash();
        Self {
            document_id: NEXT_DOCUMENT_ID.fetch_add(1, Ordering::Relaxed),
            revision: 0,
            document,
            history: Vec::new(),
            redo_stack: Vec::new(),
            history_estimated_bytes: 0,
            redo_estimated_bytes: 0,
            can_undo: false,
            can_redo: false,
            current_content_hash: content_hash,
            saved_content_hash: content_hash,
            search_index: SearchIndexStore::default(),
        }
    }

    pub fn file_data(&self) -> &FileData {
        self.document.projection()
    }

    pub fn document_id(&self) -> u64 {
        self.document_id
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn formula_status(&self) -> FormulaStatus {
        self.document.formula_status()
    }

    pub fn capabilities(&self) -> WorkbookCapabilities {
        self.document.capabilities()
    }

    pub fn search_index_stamp(&self) -> SearchIndexStamp {
        self.search_index.stamp(self.document_id)
    }

    pub fn install_search_index(
        &mut self,
        sheet_index: usize,
        stamp: SearchIndexStamp,
        index: Option<SearchSheetIndex>,
    ) {
        self.search_index
            .install_sheet_index(self.document_id, sheet_index, stamp, index);
        self.search_index.truncate(self.file_data().sheets.len());
    }

    pub fn mark_search_index_stale(&mut self) -> SearchIndexStamp {
        self.search_index.mark_stale(self.document_id)
    }

    pub fn mark_search_sheets_stale(&mut self, sheet_indexes: impl IntoIterator<Item = usize>) {
        for sheet_index in sheet_indexes {
            self.search_index.mark_sheet_stale(sheet_index);
        }
    }

    pub fn mark_search_sheet_fresh(&mut self, sheet_index: usize, stamp: SearchIndexStamp) {
        self.search_index
            .mark_sheet_fresh(self.document_id, sheet_index, stamp);
    }

    pub fn search_writer_handle(
        &self,
        sheet_index: usize,
        stamp: SearchIndexStamp,
    ) -> Option<SearchWriterHandle> {
        self.search_index
            .writer_handle(self.document_id, sheet_index, stamp)
    }

    pub fn indexed_search_sheet(
        &self,
        sheet_index: usize,
        query: &str,
        limit: usize,
    ) -> Option<Vec<CellPosition>> {
        self.search_index.search_sheet(sheet_index, query, limit)
    }

    pub fn search_sheet_snapshot(&self, sheet_index: usize) -> Option<SearchSheetSnapshot> {
        let sheet = self.file_data().sheets.get(sheet_index)?;
        Some(SearchSheetSnapshot {
            sheet_index,
            sheet_name: sheet.name.clone(),
            cells: sheet
                .rows
                .iter()
                .enumerate()
                .flat_map(|(row_idx, row)| {
                    row.iter().enumerate().filter_map(move |(col_idx, cell)| {
                        let text = cell.to_display_string();
                        (!text.is_empty()).then_some(SearchCellSnapshot {
                            row: row_idx,
                            col: col_idx,
                            text,
                        })
                    })
                })
                .collect(),
        })
    }

    pub fn is_dirty(&self) -> bool {
        self.current_content_hash != self.saved_content_hash
    }

    pub fn mark_saved(&mut self) {
        self.refresh_content_hash();
        self.saved_content_hash = self.current_content_hash;
    }

    pub fn generate_file_bytes_for_target(
        &self,
        target_path_or_name: &str,
    ) -> Result<(String, Vec<u8>), AppError> {
        self.document
            .generate_file_bytes_for_target(target_path_or_name)
    }

    /// 执行命令并记录到历史，返回增量结果。
    pub fn execute(&mut self, command: EditorCommand) -> Result<ExecutedOperation, AppError> {
        let operation = command.resolve(self.file_data())?;
        self.ensure_operation_supported(&operation)?;
        let should_mark_search_stale = operation.requires_search_rebuild();
        let before = self.document.capture_memento_side(&operation);
        if operation.is_noop() {
            let result = self.document.execute_operation(&operation, &before)?;
            self.update_flags();
            self.refresh_content_hash();
            return Ok(ExecutedOperation {
                operation: Some(result.operation),
                cell_changes: result.cell_changes,
                restore: None,
            });
        }

        let result = self.document.execute_operation(&operation, &before)?;
        let after = self.document.capture_memento_side(&operation);
        let memento = SpreadsheetDocument::create_memento(before, after);
        let stale_sheets = operation.search_stale_sheets(&result.cell_changes);
        let operation_result = result.operation;
        let cell_changes = result.cell_changes;
        self.push_history(HistoryEntry::new(memento, operation_result.clone()));
        self.clear_redo_stack();
        self.bump_revision();
        if should_mark_search_stale {
            self.mark_search_index_stale();
        } else {
            self.mark_search_sheets_stale(stale_sheets);
        }
        self.update_flags();
        self.refresh_content_hash();
        Ok(ExecutedOperation {
            operation: Some(operation_result),
            cell_changes,
            restore: None,
        })
    }

    /// 撤销上一个操作
    pub fn undo(&mut self) -> Result<Option<ExecutedOperation>, AppError> {
        if let Some(entry) = self.history.pop() {
            self.history_estimated_bytes = self
                .history_estimated_bytes
                .saturating_sub(entry.estimated_bytes);
            let restore = self
                .document
                .restore_memento(&entry.memento, MementoSide::Before)?;
            let operation = entry.operation.clone();
            self.push_redo(entry);
            self.bump_revision();
            self.mark_search_index_stale();
            self.update_flags();
            self.refresh_content_hash();
            Ok(Some(ExecutedOperation {
                operation: Some(operation),
                cell_changes: Vec::new(),
                restore: Some(restore),
            }))
        } else {
            Ok(None)
        }
    }

    /// 重做上一个被撤销的操作
    pub fn redo(&mut self) -> Result<Option<ExecutedOperation>, AppError> {
        if let Some(entry) = self.redo_stack.pop() {
            self.redo_estimated_bytes = self
                .redo_estimated_bytes
                .saturating_sub(entry.estimated_bytes);
            let restore = self
                .document
                .restore_memento(&entry.memento, MementoSide::After)?;
            let operation = entry.operation.clone();
            self.push_history(entry);
            self.bump_revision();
            self.mark_search_index_stale();
            self.update_flags();
            self.refresh_content_hash();
            Ok(Some(ExecutedOperation {
                operation: Some(operation),
                cell_changes: Vec::new(),
                restore: Some(restore),
            }))
        } else {
            Ok(None)
        }
    }

    fn update_flags(&mut self) {
        self.can_undo = !self.history.is_empty();
        self.can_redo = !self.redo_stack.is_empty();
    }

    fn refresh_content_hash(&mut self) {
        self.current_content_hash = self.document.content_hash();
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    fn push_history(&mut self, entry: HistoryEntry) {
        self.history_estimated_bytes += entry.estimated_bytes;
        self.history.push(entry);
        while self.history.len() > MAX_HISTORY_ENTRIES
            || self.history_estimated_bytes > MAX_HISTORY_BYTES
        {
            let evicted = self.history.remove(0);
            self.history_estimated_bytes = self
                .history_estimated_bytes
                .saturating_sub(evicted.estimated_bytes);
            if self.history.is_empty() {
                break;
            }
        }
    }

    fn clear_redo_stack(&mut self) {
        self.redo_stack.clear();
        self.redo_estimated_bytes = 0;
    }

    fn push_redo(&mut self, entry: HistoryEntry) {
        self.redo_estimated_bytes += entry.estimated_bytes;
        self.redo_stack.push(entry);
        while self.redo_stack.len() > MAX_HISTORY_ENTRIES
            || self.redo_estimated_bytes > MAX_HISTORY_BYTES
        {
            let evicted = self.redo_stack.remove(0);
            self.redo_estimated_bytes = self
                .redo_estimated_bytes
                .saturating_sub(evicted.estimated_bytes);
            if self.redo_stack.is_empty() {
                break;
            }
        }
    }

    fn ensure_operation_supported(
        &self,
        operation: &crate::ops::AppliedOperation,
    ) -> Result<(), AppError> {
        let capabilities = self.capabilities();
        if operation.is_row_structure_change() && !capabilities.can_insert_delete_rows {
            return Err(AppError::UnsupportedWorkbookStructure(
                capabilities.blocked_row_structure_reasons.join(", "),
            ));
        }
        if operation.is_column_structure_change() && !capabilities.can_insert_delete_columns {
            return Err(AppError::UnsupportedWorkbookStructure(
                capabilities.blocked_column_structure_reasons.join(", "),
            ));
        }
        if operation.is_sheet_structure_change() && !capabilities.can_insert_delete_sheets {
            return Err(AppError::UnsupportedWorkbookStructure(
                capabilities.blocked_sheet_structure_reasons.join(", "),
            ));
        }
        if operation.is_layout_change() && !capabilities.can_resize_rows_columns {
            return Err(AppError::UnsupportedWorkbookStructure(
                capabilities.blocked_resize_reasons.join(", "),
            ));
        }
        if operation.is_cell_edit() && !capabilities.can_edit_cells {
            return Err(AppError::UnsupportedWorkbookStructure(
                capabilities.blocked_edit_reasons.join(", "),
            ));
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
    fn structure_formula_rewrite_skips_are_reported_in_diagnostics() {
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
        state
            .execute(EditorCommand::AddRow {
                sheet_index: 0,
                row_index: 0,
            })
            .expect("add row");

        match state.formula_status() {
            FormulaStatus::Ready { diagnostics } => {
                assert_eq!(diagnostics.skipped_reference_rewrite_count, 1);
            }
            status => panic!("expected ready status, got {status:?}"),
        }
        let (_, saved_bytes) = state
            .generate_file_bytes_for_target("skipped-rewrite.xlsx")
            .expect("save workbook");
        let saved = reader::xlsx::read_reader(Cursor::new(saved_bytes), true).expect("read saved");
        assert_eq!(
            saved
                .sheet(1)
                .expect("sheet")
                .cell("A1")
                .expect("A1")
                .formula(),
            "SUM("
        );
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
        assert!(!capabilities.can_edit_cells);
        assert!(!capabilities.can_resize_rows_columns);
        assert!(!capabilities.can_insert_delete_rows);
        assert!(!capabilities.can_insert_delete_columns);
        assert!(capabilities.can_insert_delete_sheets);
        assert!(
            capabilities
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

        assert!(state.can_undo);
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

        assert_eq!(state.history.len(), MAX_HISTORY_ENTRIES);
        assert!(state.history_estimated_bytes <= MAX_HISTORY_BYTES);
        assert!(state.can_undo);
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

        assert!(state.history.len() < 40);
        assert!(state.history_estimated_bytes <= MAX_HISTORY_BYTES);
        assert!(state.can_undo);
    }
}
