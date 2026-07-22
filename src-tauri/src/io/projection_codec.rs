use crate::document::backing::workbook_port::WorkbookBackingPort;
use crate::document_data::{DocumentData, DocumentSheet};
use crate::domain::CellValue;
use crate::error::AppError;
use crate::io::codec::writer;
use crate::io::layout_units::{px_to_excel_column_width, px_to_points};
use crate::io::projection_mapper::ProjectionMapper;

use umya_spreadsheet::{Workbook, Worksheet};

pub(crate) struct WorkbookProjectionCodec;

impl WorkbookProjectionCodec {
    pub(crate) fn read_sheets(workbook: &Workbook) -> Vec<DocumentSheet> {
        ProjectionMapper::sheets_from_workbook(workbook)
    }

    pub(crate) fn refresh_projection(workbook: &Workbook, projection: &mut DocumentData) {
        ProjectionMapper::refresh_file_data_from_workbook(workbook, projection);
    }

    pub(crate) fn sync_merge_ranges(
        workbook: &mut Workbook,
        projection: &DocumentData,
    ) -> Result<(), AppError> {
        ProjectionMapper::sync_merge_ranges_to_workbook(workbook, projection)
    }

    pub(crate) fn validate(workbook: &Workbook, projection: &DocumentData) -> Result<(), AppError> {
        ProjectionMapper::validate_workbook_matches_projection(workbook, projection)
    }

    pub(crate) fn validate_sheets(
        workbook: &Workbook,
        projection: &DocumentData,
        sheet_indexes: impl IntoIterator<Item = usize>,
    ) -> Result<(), AppError> {
        ProjectionMapper::validate_workbook_sheets_match_projection(
            workbook,
            projection,
            sheet_indexes,
        )
    }
}

impl WorkbookBackingPort for WorkbookProjectionCodec {
    fn refresh_projection(&self, workbook: &Workbook, projection: &mut DocumentData) {
        Self::refresh_projection(workbook, projection);
    }

    fn sync_merge_ranges(
        &self,
        workbook: &mut Workbook,
        projection: &DocumentData,
    ) -> Result<(), AppError> {
        Self::sync_merge_ranges(workbook, projection)
    }

    fn validate_projection(
        &self,
        workbook: &Workbook,
        projection: &DocumentData,
    ) -> Result<(), AppError> {
        Self::validate(workbook, projection)
    }

    fn validate_projection_sheets(
        &self,
        workbook: &Workbook,
        projection: &DocumentData,
        sheet_indexes: &[usize],
    ) -> Result<(), AppError> {
        Self::validate_sheets(workbook, projection, sheet_indexes.iter().copied())
    }

    fn sync_sheet(&self, worksheet: &mut Worksheet, sheet: &DocumentSheet) -> Result<(), AppError> {
        writer::sync_sheet_from_sheet_data(worksheet, sheet)
    }

    fn write_cell(&self, worksheet: &mut Worksheet, row: u32, col: u32, value: &CellValue) {
        writer::write_cell(worksheet, row, col, value);
    }

    fn column_width_from_pixels(&self, pixels: u32) -> f64 {
        px_to_excel_column_width(pixels)
    }

    fn row_height_from_pixels(&self, pixels: u32) -> f64 {
        px_to_points(pixels)
    }
}
