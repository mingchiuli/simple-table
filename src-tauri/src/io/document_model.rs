use std::path::Path;

use crate::error::AppError;
use crate::formula::engine::FormulaRuntime;
use crate::io::codec::writer;
use crate::io::workbook_state;
use crate::ops::Operation;
use crate::state::content_hash::{ContentHash, hash_file_content};
use crate::types::{CellValue, FileData, OperationResult, SheetCellChange};
use umya_spreadsheet::Workbook;

#[derive(Debug, Clone)]
pub struct DocumentOperationResult {
    pub operation: OperationResult,
    pub cell_changes: Vec<SheetCellChange>,
}

enum SpreadsheetDocumentBody {
    Excel { workbook: Workbook },
    ProjectionOnly,
}

/// Canonical spreadsheet document.
///
/// Excel files keep the original `Workbook` as the persistence object. `FileData`
/// is a projection used by UI, formula calculation, search, and dirty hashing.
pub struct SpreadsheetDocument {
    projection: FileData,
    body: SpreadsheetDocumentBody,
    formula_runtime: FormulaRuntime,
}

impl SpreadsheetDocument {
    pub fn new(mut projection: FileData, workbook: Option<Workbook>) -> Self {
        let formula_runtime = FormulaRuntime::new(&mut projection).unwrap_or_else(|error| {
            eprintln!("Formula runtime initialization failed: {error}");
            FormulaRuntime::empty()
        });

        Self {
            projection,
            body: workbook
                .map(|workbook| SpreadsheetDocumentBody::Excel { workbook })
                .unwrap_or(SpreadsheetDocumentBody::ProjectionOnly),
            formula_runtime,
        }
    }

    pub fn projection(&self) -> &FileData {
        &self.projection
    }

    pub fn content_hash(&self) -> ContentHash {
        hash_file_content(&self.projection)
    }

    pub fn generate_file_bytes_for_target(
        &self,
        target_path_or_name: &str,
    ) -> Result<(String, Vec<u8>), AppError> {
        let extension = Path::new(target_path_or_name)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_lowercase())
            .unwrap_or_else(|| "xlsx".to_string());

        match extension.as_str() {
            "xlsx" | "xlsm" => match &self.body {
                SpreadsheetDocumentBody::Excel { workbook } => {
                    writer::generate_excel_bytes_from_workbook_for_target(
                        workbook,
                        target_path_or_name,
                    )
                }
                SpreadsheetDocumentBody::ProjectionOnly => {
                    writer::generate_file_bytes_for_target(&self.projection, target_path_or_name)
                }
            },
            "csv" => writer::generate_file_bytes_for_target(&self.projection, target_path_or_name),
            _ => Err(AppError::UnsupportedFormat),
        }
    }

    pub fn execute_operation(
        &mut self,
        operation: &Operation,
    ) -> Result<DocumentOperationResult, AppError> {
        let previous_projection = self.projection.clone();
        let previous_body = self.clone_body();

        let result = operation.execute(&mut self.projection);

        if let Err(error) = self.patch_workbook_after_operation(operation, &result, &[]) {
            self.projection = previous_projection;
            self.body = previous_body;
            self.rebuild_formula_runtime();
            return Err(error);
        }

        let cell_changes = self.recalculate_after_operation(operation);

        if !cell_changes.is_empty()
            && let Err(error) = self.patch_workbook_formula_changes(&cell_changes)
        {
            self.projection = previous_projection;
            self.body = previous_body;
            self.rebuild_formula_runtime();
            return Err(error);
        }

        Ok(DocumentOperationResult {
            operation: result,
            cell_changes,
        })
    }

    fn recalculate_after_operation(&mut self, operation: &Operation) -> Vec<SheetCellChange> {
        match operation {
            Operation::SetCell {
                sheet_index,
                row,
                col,
                new_value,
                ..
            } => {
                let result = self.formula_runtime.sync_cell_and_recalculate(
                    &mut self.projection,
                    *sheet_index,
                    *row,
                    *col,
                );

                match result {
                    Ok(changes) => changes,
                    Err(error) => {
                        eprintln!("Formula recalculation failed: {error}");
                        let changes = self.formula_error_change(
                            *sheet_index,
                            *row,
                            *col,
                            new_value,
                            error.to_string(),
                        );
                        self.rebuild_formula_runtime();
                        changes
                    }
                }
            }
            Operation::SetColumnWidth { .. } | Operation::SetRowHeight { .. } => Vec::new(),
            _ => match self.formula_runtime.rebuild(&mut self.projection) {
                Ok(changes) => changes,
                Err(error) => {
                    eprintln!("Formula recalculation failed: {error}");
                    self.rebuild_formula_runtime();
                    Vec::new()
                }
            },
        }
    }

    fn formula_error_change(
        &mut self,
        sheet_index: usize,
        row: usize,
        col: usize,
        value: &CellValue,
        error: String,
    ) -> Vec<SheetCellChange> {
        if !matches!(value, CellValue::Formula { .. }) {
            return Vec::new();
        }

        let Some(cell) = self
            .projection
            .sheets
            .get_mut(sheet_index)
            .and_then(|sheet| sheet.rows.get_mut(row))
            .and_then(|row_data| row_data.get_mut(col))
        else {
            return Vec::new();
        };

        *cell = cell.with_formula_result(CellValue::Null, Some(error));
        vec![SheetCellChange {
            sheet_index,
            row,
            col,
            value: cell.clone(),
        }]
    }

    fn rebuild_formula_runtime(&mut self) {
        if let Err(error) = self.formula_runtime.rebuild(&mut self.projection) {
            eprintln!("Formula runtime rebuild failed: {error}");
            self.formula_runtime = FormulaRuntime::empty();
        }
    }

    fn patch_workbook_after_operation(
        &mut self,
        operation: &Operation,
        result: &OperationResult,
        cell_changes: &[SheetCellChange],
    ) -> Result<(), AppError> {
        match &mut self.body {
            SpreadsheetDocumentBody::Excel { workbook } => workbook_state::patch_after_operation(
                workbook,
                &mut self.projection,
                operation,
                result,
                cell_changes,
            )
            .map_err(|error| AppError::WorkbookPatchFailed(error.to_string())),
            SpreadsheetDocumentBody::ProjectionOnly => Ok(()),
        }
    }

    fn patch_workbook_formula_changes(
        &mut self,
        cell_changes: &[SheetCellChange],
    ) -> Result<(), AppError> {
        match &mut self.body {
            SpreadsheetDocumentBody::Excel { workbook } => {
                workbook_state::patch_formula_changes(workbook, &mut self.projection, cell_changes)
                    .map_err(|error| AppError::WorkbookPatchFailed(error.to_string()))
            }
            SpreadsheetDocumentBody::ProjectionOnly => Ok(()),
        }
    }

    fn clone_body(&self) -> SpreadsheetDocumentBody {
        match &self.body {
            SpreadsheetDocumentBody::Excel { workbook } => SpreadsheetDocumentBody::Excel {
                workbook: workbook.clone(),
            },
            SpreadsheetDocumentBody::ProjectionOnly => SpreadsheetDocumentBody::ProjectionOnly,
        }
    }
}
