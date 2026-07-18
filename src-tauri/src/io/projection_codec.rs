use crate::document_data::{DocumentData, DocumentSheet};
use crate::error::AppError;
use crate::io::projection_mapper::ProjectionMapper;

use umya_spreadsheet::Workbook;

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
