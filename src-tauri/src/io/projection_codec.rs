use crate::error::AppError;
use crate::io::codec::writer;
use crate::io::projection_mapper::ProjectionMapper;
use crate::types::{FileData, SheetData};
use umya_spreadsheet::Workbook;

pub(crate) struct WorkbookProjectionCodec;

impl WorkbookProjectionCodec {
    pub(crate) fn read_sheets(workbook: &Workbook) -> Vec<SheetData> {
        ProjectionMapper::sheets_from_workbook(workbook)
    }

    pub(crate) fn refresh_projection(workbook: &Workbook, projection: &mut FileData) {
        ProjectionMapper::refresh_file_data_from_workbook(workbook, projection);
    }

    #[allow(dead_code)]
    pub(crate) fn apply_projection(
        workbook: &mut Workbook,
        projection: &FileData,
    ) -> Result<(), AppError> {
        writer::sync_workbook_from_file_data(workbook, projection)
    }

    pub(crate) fn sync_merge_ranges(
        workbook: &mut Workbook,
        projection: &FileData,
    ) -> Result<(), AppError> {
        ProjectionMapper::sync_merge_ranges_to_workbook(workbook, projection)
    }

    pub(crate) fn validate(workbook: &Workbook, projection: &FileData) -> Result<(), AppError> {
        ProjectionMapper::validate_workbook_matches_projection(workbook, projection)
    }

    pub(crate) fn validate_sheets(
        workbook: &Workbook,
        projection: &FileData,
        sheet_indexes: impl IntoIterator<Item = usize>,
    ) -> Result<(), AppError> {
        ProjectionMapper::validate_workbook_sheets_match_projection(
            workbook,
            projection,
            sheet_indexes,
        )
    }
}
