use std::{collections::BTreeSet, sync::Arc};

use crate::error::AppError;
use crate::formula::ast::FormulaAstService;
use crate::io::codec::writer;
use crate::io::file_format::{SpreadsheetFileFormat, extension_of};
use crate::io::projection_codec::WorkbookProjectionCodec;
use crate::io::workbook_state::{self, StructurePatchDiagnostics};
use crate::ops::AppliedOperation;
use crate::types::{
    AppliedOperationResult, FileData, SheetCapabilities, SheetCellChange, WorkbookCapabilities,
    WorkbookSaveCapabilities, WorkbookStructureCapabilities,
};
use umya_spreadsheet::{Workbook, Worksheet};

pub enum SpreadsheetDocumentBody {
    Excel(ExcelDocumentBody),
    Csv,
    GeneratedWorkbook,
}

pub struct ExcelDocumentBody {
    workbook: Arc<Workbook>,
}

fn excel_workbook(body: &ExcelDocumentBody) -> &Workbook {
    body.workbook.as_ref()
}

fn excel_workbook_mut(body: &mut ExcelDocumentBody) -> &mut Workbook {
    Arc::make_mut(&mut body.workbook)
}

pub struct SpreadsheetDocumentBodySnapshot {
    body: SpreadsheetDocumentBody,
}

pub enum BodyStructureMemento {
    ExcelWorksheetSnapshots {
        sheet_count: usize,
        replace_tail_from: Option<usize>,
        sheets: Vec<WorksheetSnapshot>,
        estimated_bytes: usize,
    },
    ProjectionOnly,
}

pub struct WorksheetSnapshot {
    sheet_index: usize,
    worksheet: Box<Worksheet>,
}

pub struct BodyStructureOperationResult {
    pub result: AppliedOperationResult,
    pub diagnostics: StructurePatchDiagnostics,
}

pub struct BodySheetShape {
    pub sheet_index: usize,
    pub row_lengths: Vec<usize>,
    pub protected_cells: Vec<(usize, usize)>,
}

impl BodyStructureMemento {
    pub fn estimated_bytes(&self) -> usize {
        match self {
            BodyStructureMemento::ExcelWorksheetSnapshots {
                estimated_bytes, ..
            } => *estimated_bytes,
            BodyStructureMemento::ProjectionOnly => 0,
        }
    }
}

impl SpreadsheetDocumentBody {
    pub fn from_projection(projection: &FileData, workbook: Option<Workbook>) -> Self {
        match workbook {
            Some(workbook) => Self::Excel(ExcelDocumentBody {
                workbook: Arc::new(workbook),
            }),
            None if is_csv_document(projection) => Self::Csv,
            None => Self::GeneratedWorkbook,
        }
    }

    pub fn capture_structure_memento(
        &self,
        operation: &AppliedOperation,
        formula_sheet_indexes: Vec<usize>,
    ) -> BodyStructureMemento {
        match self {
            Self::Excel(body) => capture_excel_structure_memento(
                excel_workbook(body),
                operation,
                formula_sheet_indexes,
            ),
            Self::Csv | Self::GeneratedWorkbook => BodyStructureMemento::ProjectionOnly,
        }
    }

    pub fn estimate_structure_memento_bytes(
        &self,
        operation: &AppliedOperation,
        formula_sheet_indexes: Vec<usize>,
    ) -> usize {
        match self {
            Self::Excel(body) => estimate_excel_structure_memento_bytes(
                excel_workbook(body),
                operation,
                formula_sheet_indexes,
            ),
            Self::Csv | Self::GeneratedWorkbook => std::mem::size_of::<BodyStructureMemento>(),
        }
    }

    pub fn restore_structure_memento(
        &mut self,
        memento: &BodyStructureMemento,
    ) -> Result<BodyRestoreAction, AppError> {
        match memento {
            BodyStructureMemento::ExcelWorksheetSnapshots {
                sheet_count,
                replace_tail_from,
                sheets,
                ..
            } => {
                let workbook = match self {
                    Self::Excel(body) => excel_workbook_mut(body),
                    Self::Csv | Self::GeneratedWorkbook => {
                        return Ok(BodyRestoreAction::RestoreProjectionOnly);
                    }
                };
                restore_excel_structure_memento(
                    workbook,
                    *sheet_count,
                    *replace_tail_from,
                    sheets,
                )?;
                Ok(BodyRestoreAction::RefreshProjectionFromWorkbook)
            }
            BodyStructureMemento::ProjectionOnly => Ok(BodyRestoreAction::RestoreProjectionOnly),
        }
    }

    pub fn capabilities(&self, formula_structure_limitations: &[String]) -> WorkbookCapabilities {
        match self {
            Self::Excel(body) => workbook_state::workbook_capabilities(
                excel_workbook(body),
                formula_structure_limitations,
            ),
            Self::Csv => csv_workbook_capabilities(),
            Self::GeneratedWorkbook => WorkbookCapabilities::default(),
        }
    }

    pub fn unsupported_operation_features(
        &self,
        operation: &AppliedOperation,
        formula_structure_limitations: &[String],
    ) -> Vec<String> {
        match self {
            Self::Excel(body) => workbook_state::unsupported_operation_features(
                excel_workbook(body),
                operation,
                formula_structure_limitations,
            ),
            Self::Csv => csv_unsupported_operation_features(operation),
            Self::GeneratedWorkbook => Vec::new(),
        }
    }

    pub fn is_excel_backed(&self) -> bool {
        matches!(self, Self::Excel(_))
    }

    pub fn can_generate_without_projection(&self, target_path_or_name: &str) -> bool {
        SpreadsheetFileFormat::from_path_or_default(target_path_or_name)
            .is_some_and(SpreadsheetFileFormat::is_xlsx)
            && self.is_excel_backed()
    }

    pub fn save_snapshot(&self) -> SpreadsheetDocumentBodySnapshot {
        let body = match self {
            Self::Excel(body) => Self::Excel(ExcelDocumentBody {
                workbook: Arc::clone(&body.workbook),
            }),
            Self::Csv => Self::Csv,
            Self::GeneratedWorkbook => Self::GeneratedWorkbook,
        };
        SpreadsheetDocumentBodySnapshot { body }
    }

    pub fn generate_file_bytes_for_target(
        &self,
        projection: &FileData,
        target_path_or_name: &str,
    ) -> Result<(String, Vec<u8>), AppError> {
        match SpreadsheetFileFormat::from_path_or_default(target_path_or_name) {
            Some(SpreadsheetFileFormat::Xlsx) => match self {
                Self::Excel(body) => writer::generate_excel_bytes_from_workbook_for_target(
                    excel_workbook(body),
                    target_path_or_name,
                ),
                Self::Csv | Self::GeneratedWorkbook => {
                    writer::generate_file_bytes_for_target(projection, target_path_or_name)
                }
            },
            Some(SpreadsheetFileFormat::Csv) => {
                writer::generate_file_bytes_for_target(projection, target_path_or_name)
            }
            None => Err(AppError::UnsupportedFormat),
        }
    }

    pub fn generate_file_bytes_without_projection_for_target(
        &self,
        target_path_or_name: &str,
    ) -> Result<(String, Vec<u8>), AppError> {
        match SpreadsheetFileFormat::from_path_or_default(target_path_or_name) {
            Some(SpreadsheetFileFormat::Xlsx) => match self {
                Self::Excel(body) => writer::generate_excel_bytes_from_workbook_for_target(
                    excel_workbook(body),
                    target_path_or_name,
                ),
                Self::Csv | Self::GeneratedWorkbook => Err(AppError::Internal(
                    "projection is required to generate this document body".to_string(),
                )),
            },
            Some(SpreadsheetFileFormat::Csv) | None => Err(AppError::Internal(
                "projection-free save snapshots only support native xlsx workbooks".to_string(),
            )),
        }
    }

    pub fn apply_structure_operation(
        &mut self,
        projection: &mut FileData,
        operation: &AppliedOperation,
        ast_service: &mut FormulaAstService,
    ) -> Result<Option<BodyStructureOperationResult>, AppError> {
        if !operation.impact().is_structure_change() {
            return Ok(None);
        }

        match self {
            Self::Excel(body) => {
                let diagnostics = workbook_state::apply_structure_operation(
                    excel_workbook_mut(body),
                    operation,
                    ast_service,
                )?;
                WorkbookProjectionCodec::refresh_projection(excel_workbook(body), projection);
                WorkbookProjectionCodec::sync_merge_ranges(excel_workbook_mut(body), projection)?;
                WorkbookProjectionCodec::refresh_projection(excel_workbook(body), projection);
                Ok(Some(BodyStructureOperationResult {
                    result: operation
                        .patch_projector()
                        .projected_result_from_current_file(projection),
                    diagnostics,
                }))
            }
            Self::Csv | Self::GeneratedWorkbook => Ok(Some(BodyStructureOperationResult {
                result: operation.projection_mutation().execute(projection),
                diagnostics: StructurePatchDiagnostics::default(),
            })),
        }
    }

    pub fn patch_after_operation(
        &mut self,
        projection: &mut FileData,
        operation: &AppliedOperation,
        cell_changes: &[SheetCellChange],
    ) -> Result<(), AppError> {
        match self {
            Self::Excel(_) if operation.impact().is_structure_change() => Ok(()),
            Self::Excel(body) => workbook_state::patch_after_operation(
                excel_workbook_mut(body),
                projection,
                operation,
                cell_changes,
            ),
            Self::Csv | Self::GeneratedWorkbook => Ok(()),
        }
    }

    pub fn patch_formula_changes(
        &mut self,
        projection: &mut FileData,
        cell_changes: &[SheetCellChange],
    ) -> Result<(), AppError> {
        match self {
            Self::Excel(body) => workbook_state::patch_formula_changes(
                excel_workbook_mut(body),
                projection,
                cell_changes,
            ),
            Self::Csv | Self::GeneratedWorkbook => Ok(()),
        }
    }

    pub fn patch_layout_dimensions(
        &mut self,
        sheet_index: usize,
        column_widths: &std::collections::HashMap<usize, Option<u32>>,
        row_heights: &std::collections::HashMap<usize, Option<u32>>,
    ) -> Result<(), AppError> {
        match self {
            Self::Excel(body) => workbook_state::patch_layout_dimensions(
                excel_workbook_mut(body),
                sheet_index,
                column_widths,
                row_heights,
            ),
            Self::Csv | Self::GeneratedWorkbook => Ok(()),
        }
    }

    pub fn patch_cell_shapes(&mut self, sheet_shapes: &[BodySheetShape]) -> Result<(), AppError> {
        match self {
            Self::Excel(body) => {
                workbook_state::patch_cell_shapes(excel_workbook_mut(body), sheet_shapes)
            }
            Self::Csv | Self::GeneratedWorkbook => Ok(()),
        }
    }

    pub fn refresh_projection_from_workbook(&self, projection: &mut FileData) {
        if let Self::Excel(body) = self {
            WorkbookProjectionCodec::refresh_projection(excel_workbook(body), projection);
        }
    }

    pub fn validate_projection_consistency(&self, projection: &FileData) -> Result<(), AppError> {
        match self {
            Self::Excel(body) => {
                WorkbookProjectionCodec::validate(excel_workbook(body), projection)
            }
            Self::Csv | Self::GeneratedWorkbook => Ok(()),
        }
    }

    pub fn validate_persisted_projection_consistency(
        &self,
        projection: &FileData,
    ) -> Result<(), AppError> {
        match self {
            Self::Excel(body) => {
                WorkbookProjectionCodec::validate(excel_workbook(body), projection)
            }
            Self::Csv | Self::GeneratedWorkbook => Ok(()),
        }
    }

    pub fn validate_projection_sheets(
        &self,
        projection: &FileData,
        sheet_indexes: impl IntoIterator<Item = usize>,
    ) -> Result<(), AppError> {
        match self {
            Self::Excel(body) => WorkbookProjectionCodec::validate_sheets(
                excel_workbook(body),
                projection,
                sheet_indexes,
            ),
            Self::Csv | Self::GeneratedWorkbook => Ok(()),
        }
    }
}

impl SpreadsheetDocumentBodySnapshot {
    pub fn is_excel_backed(&self) -> bool {
        self.body.is_excel_backed()
    }

    pub fn generate_file_bytes_without_projection_for_target(
        &self,
        target_path_or_name: &str,
    ) -> Result<(String, Vec<u8>), AppError> {
        self.body
            .generate_file_bytes_without_projection_for_target(target_path_or_name)
    }

    pub fn generate_file_bytes_for_target(
        &self,
        projection: &FileData,
        target_path_or_name: &str,
    ) -> Result<(String, Vec<u8>), AppError> {
        self.body
            .generate_file_bytes_for_target(projection, target_path_or_name)
    }

    pub fn validate_persisted_projection_consistency(
        &self,
        projection: &FileData,
    ) -> Result<(), AppError> {
        self.body
            .validate_persisted_projection_consistency(projection)
    }
}

pub enum BodyRestoreAction {
    RefreshProjectionFromWorkbook,
    RestoreProjectionOnly,
}

fn capture_excel_structure_memento(
    workbook: &Workbook,
    operation: &AppliedOperation,
    formula_sheet_indexes: Vec<usize>,
) -> BodyStructureMemento {
    let sheet_count = workbook.sheet_count();
    let replace_tail_from = match operation {
        AppliedOperation::AddSheet { sheet_index, .. }
        | AppliedOperation::DeleteSheet { sheet_index } => Some(*sheet_index),
        _ => None,
    };
    let mut sheet_indexes = BTreeSet::new();
    match replace_tail_from {
        Some(start) => sheet_indexes.extend(start..sheet_count),
        None => {
            if let Some(sheet_index) =
                affected_sheet_index(operation).filter(|sheet_index| *sheet_index < sheet_count)
            {
                sheet_indexes.insert(sheet_index);
            }
        }
    }
    sheet_indexes.extend(formula_sheet_indexes);

    let sheets: Vec<WorksheetSnapshot> = sheet_indexes
        .into_iter()
        .filter_map(|sheet_index| {
            workbook
                .sheet(sheet_index)
                .ok()
                .map(|worksheet| WorksheetSnapshot {
                    sheet_index,
                    worksheet: Box::new(worksheet.clone()),
                })
        })
        .collect();
    let estimated_bytes = sheets
        .iter()
        .map(|snapshot| estimate_worksheet_bytes(&snapshot.worksheet))
        .sum::<usize>()
        + std::mem::size_of::<BodyStructureMemento>();

    BodyStructureMemento::ExcelWorksheetSnapshots {
        sheet_count,
        replace_tail_from,
        sheets,
        estimated_bytes,
    }
}

fn estimate_excel_structure_memento_bytes(
    workbook: &Workbook,
    operation: &AppliedOperation,
    formula_sheet_indexes: Vec<usize>,
) -> usize {
    affected_excel_structure_sheet_indexes(workbook, operation, formula_sheet_indexes)
        .into_iter()
        .filter_map(|sheet_index| workbook.sheet(sheet_index).ok())
        .map(estimate_worksheet_bytes)
        .sum::<usize>()
        + std::mem::size_of::<BodyStructureMemento>()
}

fn affected_excel_structure_sheet_indexes(
    workbook: &Workbook,
    operation: &AppliedOperation,
    formula_sheet_indexes: Vec<usize>,
) -> BTreeSet<usize> {
    let sheet_count = workbook.sheet_count();
    let replace_tail_from = match operation {
        AppliedOperation::AddSheet { sheet_index, .. }
        | AppliedOperation::DeleteSheet { sheet_index } => Some(*sheet_index),
        _ => None,
    };
    let mut sheet_indexes = BTreeSet::new();
    match replace_tail_from {
        Some(start) => sheet_indexes.extend(start..sheet_count),
        None => {
            if let Some(sheet_index) =
                affected_sheet_index(operation).filter(|sheet_index| *sheet_index < sheet_count)
            {
                sheet_indexes.insert(sheet_index);
            }
        }
    }
    sheet_indexes.extend(formula_sheet_indexes);
    sheet_indexes
}

fn restore_excel_structure_memento(
    workbook: &mut Workbook,
    sheet_count: usize,
    replace_tail_from: Option<usize>,
    snapshots: &[WorksheetSnapshot],
) -> Result<(), AppError> {
    if let Some(start) = replace_tail_from {
        restore_excel_sheet_slots(
            workbook,
            snapshots
                .iter()
                .filter(|snapshot| snapshot.sheet_index < start),
        )?;
        return restore_excel_sheet_tail(
            workbook,
            sheet_count,
            start,
            snapshots
                .iter()
                .filter(|snapshot| snapshot.sheet_index >= start),
        );
    }

    restore_excel_sheet_slots(workbook, snapshots.iter())
}

fn restore_excel_sheet_slots<'a>(
    workbook: &mut Workbook,
    snapshots: impl IntoIterator<Item = &'a WorksheetSnapshot>,
) -> Result<(), AppError> {
    let sheets = workbook.sheet_collection_mut();
    for snapshot in snapshots {
        let Some(slot) = sheets.get_mut(snapshot.sheet_index) else {
            return Err(AppError::UnsupportedWorkbookStructure(
                "worksheet snapshot target is missing".to_string(),
            ));
        };
        *slot = (*snapshot.worksheet).clone();
    }
    Ok(())
}

fn restore_excel_sheet_tail<'a>(
    workbook: &mut Workbook,
    sheet_count: usize,
    start: usize,
    snapshots: impl IntoIterator<Item = &'a WorksheetSnapshot>,
) -> Result<(), AppError> {
    let remove_from = start.min(workbook.sheet_count());
    while workbook.sheet_count() > remove_from {
        workbook
            .remove_sheet(remove_from)
            .map_err(|error| AppError::WriteError(error.to_string()))?;
    }

    for snapshot in snapshots {
        workbook
            .add_sheet((*snapshot.worksheet).clone())
            .map_err(|error| AppError::WriteError(error.to_string()))?;
    }

    if workbook.sheet_count() != sheet_count {
        return Err(AppError::UnsupportedWorkbookStructure(format!(
            "worksheet snapshot restore expected {sheet_count} sheets, got {}",
            workbook.sheet_count()
        )));
    }

    Ok(())
}

fn affected_sheet_index(operation: &AppliedOperation) -> Option<usize> {
    match operation {
        AppliedOperation::AddRow { sheet_index, .. }
        | AppliedOperation::DeleteRow { sheet_index, .. }
        | AppliedOperation::AddColumn { sheet_index, .. }
        | AppliedOperation::DeleteColumn { sheet_index, .. }
        | AppliedOperation::SetCell { sheet_index, .. }
        | AppliedOperation::SetColumnWidth { sheet_index, .. }
        | AppliedOperation::SetRowHeight { sheet_index, .. } => Some(*sheet_index),
        AppliedOperation::SetCells { changes } => changes.first().map(|change| change.sheet_index),
        AppliedOperation::AddSheet { .. } | AppliedOperation::DeleteSheet { .. } => None,
    }
}

fn estimate_worksheet_bytes(worksheet: &Worksheet) -> usize {
    let (highest_col, highest_row) = worksheet.highest_column_and_row();
    std::mem::size_of::<Worksheet>()
        + worksheet.name().len()
        + highest_col as usize * 16
        + highest_row as usize * 16
        + worksheet.column_dimensions().len() * 64
        + worksheet.row_dimensions().len() * 64
        + worksheet.merge_cells().len() * 48
        + worksheet
            .image_collection()
            .iter()
            .map(|image| std::mem::size_of_val(image) + image.image_data().len())
            .sum::<usize>()
        + worksheet.chart_collection().len() * 2048
        + worksheet
            .cells()
            .iter()
            .map(|cell| {
                128 + cell.value().as_ref().len()
                    + if cell.is_formula() {
                        cell.formula().len()
                    } else {
                        0
                    }
            })
            .sum::<usize>()
}

fn is_csv_document(file_data: &FileData) -> bool {
    extension_of(&file_data.file_name)
        .or_else(|| extension_of(&file_data.path))
        .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
}

fn csv_workbook_capabilities() -> WorkbookCapabilities {
    let sheet_capabilities = SheetCapabilities {
        can_resize_rows_columns: false,
        blocked_resize_reasons: vec!["CSV files do not persist row or column dimensions".into()],
        ..SheetCapabilities::default()
    };

    WorkbookCapabilities {
        save: WorkbookSaveCapabilities {
            detected_features: vec!["csv single-sheet value format".into()],
            ..WorkbookSaveCapabilities::default()
        },
        structure: WorkbookStructureCapabilities {
            can_insert_delete_sheets: false,
            blocked_sheet_structure_reasons: vec!["CSV files only persist one sheet".into()],
            blocked_structure_reasons: vec!["CSV files only persist one sheet".into()],
        },
        sheets: vec![sheet_capabilities],
        ..WorkbookCapabilities::default()
    }
}

fn csv_unsupported_operation_features(operation: &AppliedOperation) -> Vec<String> {
    if operation.impact().is_layout_change() {
        return vec!["CSV files do not persist row or column dimensions".into()];
    }
    if operation.impact().is_sheet_structure_change() {
        return vec!["CSV files only persist one sheet".into()];
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use umya_spreadsheet::structs::Image;
    use umya_spreadsheet::structs::drawing::spreadsheet::MarkerType;

    #[test]
    fn worksheet_estimate_includes_actual_image_bytes() {
        let mut worksheet = Worksheet::default();
        let baseline = estimate_worksheet_bytes(&worksheet);
        let image_bytes = vec![0x5a; 2 * 1024 * 1024];
        let mut image = Image::default();
        image.new_image_with_dimensions(
            16,
            16,
            "large.png",
            image_bytes.clone(),
            MarkerType::default(),
        );
        worksheet.add_image(image);

        assert!(estimate_worksheet_bytes(&worksheet) >= baseline + image_bytes.len());
    }
}
