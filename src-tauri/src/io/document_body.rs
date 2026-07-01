use std::path::Path;

use crate::error::AppError;
use crate::io::codec::reader::read_workbook_from_xlsx_bytes;
use crate::io::codec::writer;
use crate::io::workbook_state;
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
    ExcelWorkbookBytes { bytes: Box<[u8]> },
    ExcelWorkbookClone { workbook: Box<Workbook> },
    ProjectionOnly,
}

impl BodyStructureMemento {
    pub fn estimated_bytes(&self) -> usize {
        match self {
            BodyStructureMemento::ExcelWorkbookBytes { bytes } => bytes.len(),
            BodyStructureMemento::ExcelWorkbookClone { workbook } => {
                estimate_workbook_bytes(workbook)
            }
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
            Self::Excel(body) => match writer::write_workbook_to_bytes(&body.workbook) {
                Ok(bytes) => BodyStructureMemento::ExcelWorkbookBytes {
                    bytes: bytes.into_boxed_slice(),
                },
                Err(_) => BodyStructureMemento::ExcelWorkbookClone {
                    workbook: body.workbook.clone(),
                },
            },
            Self::Csv | Self::GeneratedWorkbook => BodyStructureMemento::ProjectionOnly,
        }
    }

    pub fn restore_structure_memento(
        &mut self,
        memento: &BodyStructureMemento,
    ) -> Result<BodyRestoreAction, AppError> {
        match memento {
            BodyStructureMemento::ExcelWorkbookBytes { bytes } => {
                let workbook = read_workbook_from_xlsx_bytes(bytes.to_vec())?;
                *self = Self::Excel(ExcelDocumentBody {
                    workbook: Box::new(workbook),
                });
                Ok(BodyRestoreAction::RefreshProjectionFromWorkbook)
            }
            BodyStructureMemento::ExcelWorkbookClone { workbook } => {
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
    ) -> Result<bool, AppError> {
        match self {
            Self::Excel(body) if operation.is_structure_change() => {
                workbook_state::apply_structure_operation(&mut body.workbook, operation)?;
                Ok(true)
            }
            Self::Excel(_) | Self::Csv | Self::GeneratedWorkbook => Ok(false),
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
    writer::write_workbook_to_bytes(workbook)
        .map(|bytes| bytes.len())
        .unwrap_or(8 * 1024 * 1024)
}

fn is_csv_document(file_data: &FileData) -> bool {
    Path::new(&file_data.file_name)
        .extension()
        .or_else(|| Path::new(&file_data.path).extension())
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
}
