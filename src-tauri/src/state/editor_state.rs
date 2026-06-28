use crate::error::AppError;
use crate::io::document_model::SpreadsheetDocument;
use crate::ops::Operation;
use crate::state::content_hash::ContentHash;
use crate::state::search_index::{
    SearchIndexStamp, SearchIndexStore, SearchSheetIndex, SearchWriterHandle,
};
use crate::types::{CellPosition, CellValue, FileData, OperationResult, SheetCellChange};
use umya_spreadsheet::Workbook;

#[derive(Debug, Clone)]
pub struct ExecutedOperation {
    pub operation: OperationResult,
    pub cell_changes: Vec<SheetCellChange>,
}

/// 编辑器状态管理器
pub struct EditorState {
    document: SpreadsheetDocument,
    pub history: Vec<Operation>,
    pub redo_stack: Vec<Operation>,
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
            document,
            history: Vec::new(),
            redo_stack: Vec::new(),
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

    pub fn search_index_stamp(&self) -> SearchIndexStamp {
        self.search_index.stamp()
    }

    pub fn install_search_index(
        &mut self,
        sheet_index: usize,
        stamp: SearchIndexStamp,
        index: Option<SearchSheetIndex>,
    ) {
        self.search_index
            .install_sheet_index(sheet_index, stamp, index);
        self.search_index.truncate(self.file_data().sheets.len());
    }

    pub fn mark_search_index_stale(&mut self) -> SearchIndexStamp {
        self.search_index.mark_stale()
    }

    pub fn search_sheet(&self, sheet_index: usize, query: &str, limit: usize) -> Vec<CellPosition> {
        self.search_index
            .search_sheet(sheet_index, query, limit)
            .unwrap_or_else(|| self.scan_sheet(sheet_index, query, limit))
    }

    pub fn search_writer_handle(
        &self,
        sheet_index: usize,
        stamp: SearchIndexStamp,
    ) -> Option<SearchWriterHandle> {
        self.search_index.writer_handle(sheet_index, stamp)
    }

    fn scan_sheet(&self, sheet_index: usize, query: &str, limit: usize) -> Vec<CellPosition> {
        let query = query.trim();
        if query.is_empty() || limit == 0 {
            return Vec::new();
        }

        let Some(sheet) = self.file_data().sheets.get(sheet_index) else {
            return Vec::new();
        };
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();
        for (row_idx, row) in sheet.rows.iter().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
                if cell
                    .to_display_string()
                    .to_lowercase()
                    .contains(&query_lower)
                {
                    results.push(CellPosition {
                        row: row_idx,
                        col: col_idx,
                    });
                    if results.len() >= limit {
                        return results;
                    }
                }
            }
        }
        results
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

    /// 执行操作并记录到历史，返回增量结果
    pub fn execute(&mut self, mut operation: Operation) -> Result<ExecutedOperation, AppError> {
        // 在执行操作前，先准备好需要的数据，以便撤销/重做
        match &operation {
            // SetCell: 从 file_data 中获取真正的旧值，而不是依赖前端传入的（可能已过时）
            Operation::SetCell {
                sheet_index,
                row,
                col,
                old_value,
                new_value,
            } => {
                if let Some(sheet) = self.file_data().sheets.get(*sheet_index)
                    && let Some(real_old) = sheet.rows.get(*row).and_then(|r| r.get(*col))
                {
                    // 如果新值和旧值相同，不需要记录到 history
                    if real_old == new_value {
                        // 返回结果但不记录到 history
                        let result = self.document.execute_operation(&operation)?;
                        self.update_flags();
                        self.refresh_content_hash();
                        return Ok(ExecutedOperation {
                            operation: result.operation,
                            cell_changes: result.cell_changes,
                        });
                    }
                    // 只有当后端获取的旧值与前端传入的不同时，才更新 operation
                    if real_old != old_value {
                        operation = Operation::SetCell {
                            sheet_index: *sheet_index,
                            row: *row,
                            col: *col,
                            old_value: real_old.clone(),
                            new_value: new_value.clone(),
                        };
                    }
                }
            }
            // AddColumn: 添加空列，需要补充列索引（只当 col_data 为空时）
            Operation::AddColumn {
                sheet_index,
                col_index,
                col_data: _,
                column_width,
            } => {
                // 如果 col_data 已有数据（撤销操作），保留原数据
                if col_index.is_none() && *sheet_index < self.file_data().sheets.len() {
                    // 正常添加列，补充列索引
                    if let Some(sheet) = self.file_data().sheets.get(*sheet_index) {
                        let col_count = sheet.rows.first().map(|r| r.len()).unwrap_or(0);
                        // 新列被添加到末尾，索引就是当前列数（添加前）
                        operation = Operation::AddColumn {
                            sheet_index: *sheet_index,
                            col_index: Some(col_count),
                            col_data: vec![],
                            column_width: *column_width,
                        };
                    }
                }
            }
            // AddRow: 添加空行，需要补充行数据（只当 row_data 为空时）
            Operation::AddRow {
                sheet_index,
                row_index,
                row_data,
                row_height,
            } => {
                // 如果 row_data 已有数据（撤销操作），保留原数据
                if row_data.is_empty()
                    && *sheet_index < self.file_data().sheets.len()
                    && let Some(sheet) = self.file_data().sheets.get(*sheet_index)
                {
                    let col_count = sheet.rows.first().map(|r| r.len()).unwrap_or(0);
                    operation = Operation::AddRow {
                        sheet_index: *sheet_index,
                        row_index: *row_index,
                        row_data: vec![CellValue::Null; col_count],
                        row_height: *row_height,
                    };
                }
            }
            Operation::DeleteSheet {
                sheet_index,
                sheet_data,
            } => {
                // 如果 sheet_data 为空，说明是正常的删除操作，需要保存完整的 sheet 数据
                if sheet_data.is_empty()
                    && *sheet_index < self.file_data().sheets.len()
                    && let Some(removed_sheet) = self.file_data().sheets.get(*sheet_index)
                {
                    operation = Operation::DeleteSheet {
                        sheet_index: *sheet_index,
                        sheet_data: removed_sheet.clone(),
                    };
                }
            }
            Operation::SetColumnWidth {
                sheet_index,
                col_index,
                old_width,
                new_width,
            } => {
                let real_old = self
                    .file_data()
                    .sheets
                    .get(*sheet_index)
                    .and_then(|sheet| sheet.column_widths.as_ref())
                    .and_then(|widths| widths.get(col_index).copied());
                if real_old == *new_width {
                    let result = self.document.execute_operation(&operation)?;
                    self.update_flags();
                    self.refresh_content_hash();
                    return Ok(ExecutedOperation {
                        operation: result.operation,
                        cell_changes: result.cell_changes,
                    });
                }
                if real_old != *old_width {
                    operation = Operation::SetColumnWidth {
                        sheet_index: *sheet_index,
                        col_index: *col_index,
                        old_width: real_old,
                        new_width: *new_width,
                    };
                }
            }
            Operation::SetRowHeight {
                sheet_index,
                row_index,
                old_height,
                new_height,
            } => {
                let real_old = self
                    .file_data()
                    .sheets
                    .get(*sheet_index)
                    .and_then(|sheet| sheet.row_heights.as_ref())
                    .and_then(|heights| heights.get(row_index).copied());
                if real_old == *new_height {
                    let result = self.document.execute_operation(&operation)?;
                    self.update_flags();
                    self.refresh_content_hash();
                    return Ok(ExecutedOperation {
                        operation: result.operation,
                        cell_changes: result.cell_changes,
                    });
                }
                if real_old != *old_height {
                    operation = Operation::SetRowHeight {
                        sheet_index: *sheet_index,
                        row_index: *row_index,
                        old_height: real_old,
                        new_height: *new_height,
                    };
                }
            }
            // AddSheet: 提前生成名称和索引，保证 redo 重放时使用与首次执行一致的位置和名称
            Operation::AddSheet {
                name,
                sheet_data,
                sheet_index,
            } => {
                if sheet_data.is_none() {
                    let final_name = if name.is_empty() {
                        format!("Sheet{}", self.file_data().sheets.len() + 1)
                    } else {
                        name.clone()
                    };
                    let actual_index = sheet_index.unwrap_or(self.file_data().sheets.len());
                    operation = Operation::AddSheet {
                        name: final_name,
                        sheet_data: None,
                        sheet_index: Some(actual_index),
                    };
                }
            }
            _ => {}
        }

        let result = self.document.execute_operation(&operation)?;
        self.history.push(operation);
        self.redo_stack.clear();
        self.update_flags();
        self.refresh_content_hash();
        Ok(ExecutedOperation {
            operation: result.operation,
            cell_changes: result.cell_changes,
        })
    }

    /// 撤销上一个操作
    pub fn undo(&mut self) -> Result<Option<ExecutedOperation>, AppError> {
        if let Some(operation) = self.history.pop() {
            let undo_operation = operation.create_undo_op();
            let result = match self.document.execute_operation(&undo_operation) {
                Ok(result) => result,
                Err(error) => {
                    self.history.push(operation);
                    self.update_flags();
                    self.refresh_content_hash();
                    return Err(error);
                }
            };
            // 获取 redo 操作
            let redo_op = operation.create_redo_op();
            self.redo_stack.push(redo_op);

            self.update_flags();
            self.refresh_content_hash();
            Ok(Some(ExecutedOperation {
                operation: result.operation,
                cell_changes: result.cell_changes,
            }))
        } else {
            Ok(None)
        }
    }

    /// 重做上一个被撤销的操作
    pub fn redo(&mut self) -> Result<Option<ExecutedOperation>, AppError> {
        if let Some(operation) = self.redo_stack.pop() {
            let result = match self.document.execute_operation(&operation) {
                Ok(result) => result,
                Err(error) => {
                    self.redo_stack.push(operation);
                    self.update_flags();
                    self.refresh_content_hash();
                    return Err(error);
                }
            };
            self.history.push(operation);
            self.update_flags();
            self.refresh_content_hash();
            Ok(Some(ExecutedOperation {
                operation: result.operation,
                cell_changes: result.cell_changes,
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
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::io::codec::reader::read_file_with_workbook_from_bytes;
    use crate::ops::Operation;
    use serde_json::Value;
    use umya_spreadsheet::{Color, reader, writer};

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
            .execute(Operation::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                old_value: CellValue::String("old".to_string()),
                new_value: CellValue::Number(Value::from(42)),
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
            .execute(Operation::DeleteRow {
                sheet_index: 0,
                row_index: 0,
                row_data: vec![
                    CellValue::String("a1".to_string()),
                    CellValue::String("b1".to_string()),
                ],
                row_height: None,
            })
            .expect("delete row");
        state.undo().expect("undo row delete").expect("undo result");
        state.redo().expect("redo row delete").expect("redo result");
        state
            .execute(Operation::DeleteColumn {
                sheet_index: 0,
                col_index: 0,
                col_data: vec![CellValue::String("a2".to_string())],
                column_width: None,
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
            .execute(Operation::AddRow {
                sheet_index: 0,
                row_index: 1,
                row_data: vec![CellValue::Null, CellValue::Null],
                row_height: None,
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
            .execute(Operation::AddRow {
                sheet_index: 0,
                row_index: 0,
                row_data: vec![CellValue::Null],
                row_height: None,
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
            .execute(Operation::SetColumnWidth {
                sheet_index: 0,
                col_index: 0,
                old_width: None,
                new_width: Some(180),
            })
            .expect("set column width");
        state
            .execute(Operation::SetRowHeight {
                sheet_index: 0,
                row_index: 0,
                old_height: None,
                new_height: Some(96),
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
            .execute(Operation::DeleteRow {
                sheet_index: 0,
                row_index: 0,
                row_data: vec![
                    CellValue::String("a1".to_string()),
                    CellValue::String("b1".to_string()),
                ],
                row_height: Some(112),
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
            .execute(Operation::DeleteColumn {
                sheet_index: 0,
                col_index: 0,
                col_data: vec![
                    CellValue::String("a1".to_string()),
                    CellValue::String("a2".to_string()),
                ],
                column_width: Some(180),
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
}
