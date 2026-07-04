use std::{collections::BTreeSet, path::Path};

use crate::error::AppError;
use crate::io::codec::writer;
use crate::io::projection_mapper::ProjectionMapper;
use crate::io::workbook_state::{self, StructurePatchDiagnostics};
use crate::ops::AppliedOperation;
use crate::types::{AppliedOperationResult, FileData, SheetCellChange, WorkbookCapabilities};
use umya_spreadsheet::{Workbook, Worksheet};

pub enum SpreadsheetDocumentBody {
    Excel(ExcelDocumentBody),
    Csv,
    GeneratedWorkbook,
}

pub struct ExcelDocumentBody {
    workbook: Box<Workbook>,
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
                workbook: Box::new(workbook),
            }),
            None if is_csv_document(projection) => Self::Csv,
            None => Self::GeneratedWorkbook,
        }
    }

    pub fn clone_body(&self) -> Self {
        match self {
            Self::Excel(body) => Self::Excel(ExcelDocumentBody {
                workbook: body.workbook.clone(),
            }),
            Self::Csv => Self::Csv,
            Self::GeneratedWorkbook => Self::GeneratedWorkbook,
        }
    }

    pub fn capture_structure_memento(&self, operation: &AppliedOperation) -> BodyStructureMemento {
        match self {
            Self::Excel(body) => capture_excel_structure_memento(&body.workbook, operation),
            Self::Csv | Self::GeneratedWorkbook => BodyStructureMemento::ProjectionOnly,
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
                    Self::Excel(body) => &mut body.workbook,
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

    pub fn capabilities(&self) -> WorkbookCapabilities {
        match self {
            Self::Excel(body) => workbook_state::workbook_capabilities(&body.workbook),
            Self::Csv => WorkbookCapabilities {
                can_native_save: false,
                ..Default::default()
            },
            Self::GeneratedWorkbook => WorkbookCapabilities::default(),
        }
    }

    pub fn generate_file_bytes_for_target(
        &self,
        projection: &FileData,
        target_path_or_name: &str,
    ) -> Result<(String, Vec<u8>), AppError> {
        let extension = Path::new(target_path_or_name)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_lowercase())
            .unwrap_or_else(|| "xlsx".to_string());

        match extension.as_str() {
            "xlsx" => match self {
                Self::Excel(body) => writer::generate_excel_bytes_from_workbook_for_target(
                    &body.workbook,
                    target_path_or_name,
                ),
                Self::Csv | Self::GeneratedWorkbook => {
                    writer::generate_file_bytes_for_target(projection, target_path_or_name)
                }
            },
            "csv" => writer::generate_file_bytes_for_target(projection, target_path_or_name),
            _ => Err(AppError::UnsupportedFormat),
        }
    }

    pub fn apply_structure_operation(
        &mut self,
        projection: &mut FileData,
        operation: &AppliedOperation,
    ) -> Result<Option<BodyStructureOperationResult>, AppError> {
        if !operation.impact().is_structure_change() {
            return Ok(None);
        }

        match self {
            Self::Excel(body) => {
                let diagnostics =
                    workbook_state::apply_structure_operation(&mut body.workbook, operation)?;
                ProjectionMapper::refresh_file_data_from_workbook(&body.workbook, projection);
                ProjectionMapper::sync_merge_ranges_to_workbook(&mut body.workbook, projection)?;
                ProjectionMapper::refresh_file_data_from_workbook(&body.workbook, projection);
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
                &mut body.workbook,
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
            Self::Excel(body) => {
                workbook_state::patch_formula_changes(&mut body.workbook, projection, cell_changes)
            }
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
                &mut body.workbook,
                sheet_index,
                column_widths,
                row_heights,
            ),
            Self::Csv | Self::GeneratedWorkbook => Ok(()),
        }
    }

    pub fn patch_cell_shapes(
        &mut self,
        sheet_shapes: &[(usize, Vec<usize>)],
    ) -> Result<(), AppError> {
        match self {
            Self::Excel(body) => {
                workbook_state::patch_cell_shapes(&mut body.workbook, sheet_shapes)
            }
            Self::Csv | Self::GeneratedWorkbook => Ok(()),
        }
    }

    pub fn refresh_projection_from_workbook(&self, projection: &mut FileData) {
        if let Self::Excel(body) = self {
            ProjectionMapper::refresh_file_data_from_workbook(&body.workbook, projection);
        }
    }

    #[cfg(any(test, debug_assertions))]
    pub fn validate_projection_consistency(&self, projection: &FileData) -> Result<(), AppError> {
        match self {
            Self::Excel(body) => {
                ProjectionMapper::validate_workbook_matches_projection(&body.workbook, projection)
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
                ProjectionMapper::validate_workbook_matches_projection(&body.workbook, projection)
            }
            Self::Csv | Self::GeneratedWorkbook => Ok(()),
        }
    }
}

pub enum BodyRestoreAction {
    RefreshProjectionFromWorkbook,
    RestoreProjectionOnly,
}

fn capture_excel_structure_memento(
    workbook: &Workbook,
    operation: &AppliedOperation,
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
    sheet_indexes.extend(formula_sheet_indexes(workbook));

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

fn formula_sheet_indexes(workbook: &Workbook) -> Vec<usize> {
    workbook
        .sheet_collection()
        .iter()
        .enumerate()
        .filter_map(|(sheet_index, worksheet)| {
            worksheet
                .cells()
                .iter()
                .any(|cell| cell.is_formula())
                .then_some(sheet_index)
        })
        .collect()
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
        + worksheet.image_collection().len() * 1024
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
    Path::new(&file_data.file_name)
        .extension()
        .or_else(|| Path::new(&file_data.path).extension())
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
}
