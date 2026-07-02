use std::path::Path;

use crate::error::AppError;
use crate::io::codec::writer;
use crate::io::workbook_state::{self, StructurePatchDiagnostics};
use crate::ops::AppliedOperation;
use crate::types::{FileData, SheetCellChange, WorkbookCapabilities};
use umya_spreadsheet::Workbook;

pub enum SpreadsheetDocumentBody {
    Excel(ExcelDocumentBody),
    Csv,
    GeneratedWorkbook,
}

pub struct ExcelDocumentBody {
    workbook: Box<Workbook>,
}

pub enum BodyStructureMemento {
    ExcelWorkbookClone {
        workbook: Box<Workbook>,
        estimated_bytes: usize,
    },
    ProjectionOnly,
}

impl BodyStructureMemento {
    pub fn estimated_bytes(&self) -> usize {
        match self {
            BodyStructureMemento::ExcelWorkbookClone {
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

    pub fn capture_structure_memento(&self) -> BodyStructureMemento {
        match self {
            Self::Excel(body) => BodyStructureMemento::ExcelWorkbookClone {
                workbook: body.workbook.clone(),
                estimated_bytes: estimate_workbook_bytes(&body.workbook),
            },
            Self::Csv | Self::GeneratedWorkbook => BodyStructureMemento::ProjectionOnly,
        }
    }

    pub fn restore_structure_memento(
        &mut self,
        memento: &BodyStructureMemento,
    ) -> Result<BodyRestoreAction, AppError> {
        match memento {
            BodyStructureMemento::ExcelWorkbookClone { workbook, .. } => {
                *self = Self::Excel(ExcelDocumentBody {
                    workbook: workbook.clone(),
                });
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
        operation: &AppliedOperation,
    ) -> Result<Option<StructurePatchDiagnostics>, AppError> {
        match self {
            Self::Excel(body) if operation.is_structure_change() => Ok(Some(
                workbook_state::apply_structure_operation(&mut body.workbook, operation)?,
            )),
            Self::Excel(_) | Self::Csv | Self::GeneratedWorkbook => Ok(None),
        }
    }

    pub fn patch_after_operation(
        &mut self,
        projection: &mut FileData,
        operation: &AppliedOperation,
        cell_changes: &[SheetCellChange],
    ) -> Result<(), AppError> {
        match self {
            Self::Excel(_) if operation.is_structure_change() => Ok(()),
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
            workbook_state::refresh_projection_from_workbook(&body.workbook, projection);
        }
    }

    pub fn sync_all_merge_ranges_from_projection(
        &mut self,
        projection: &FileData,
    ) -> Result<(), AppError> {
        match self {
            Self::Excel(body) => workbook_state::sync_all_merge_ranges_from_projection(
                &mut body.workbook,
                projection,
            ),
            Self::Csv | Self::GeneratedWorkbook => Ok(()),
        }
    }
}

pub enum BodyRestoreAction {
    RefreshProjectionFromWorkbook,
    RestoreProjectionOnly,
}

fn estimate_workbook_bytes(workbook: &Workbook) -> usize {
    let mut bytes = std::mem::size_of::<Workbook>() + workbook.sheet_count() * 4096;
    for worksheet in workbook.sheet_collection() {
        let (highest_col, highest_row) = worksheet.highest_column_and_row();
        bytes += worksheet.name().len();
        bytes += highest_col as usize * 16;
        bytes += highest_row as usize * 16;
        bytes += worksheet.column_dimensions().len() * 64;
        bytes += worksheet.row_dimensions().len() * 64;
        bytes += worksheet.merge_cells().len() * 48;
        bytes += worksheet.image_collection().len() * 1024;
        bytes += worksheet.chart_collection().len() * 2048;
        bytes += worksheet
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
            .sum::<usize>();
    }
    bytes
}

fn is_csv_document(file_data: &FileData) -> bool {
    Path::new(&file_data.file_name)
        .extension()
        .or_else(|| Path::new(&file_data.path).extension())
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
}
