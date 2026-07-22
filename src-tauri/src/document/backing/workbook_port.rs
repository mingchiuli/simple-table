use crate::document_data::{DocumentData, DocumentSheet};
use crate::domain::CellValue;
use crate::error::AppError;
use umya_spreadsheet::{Workbook, Worksheet};

pub(crate) trait WorkbookBackingPort: Send + Sync {
    fn refresh_projection(&self, workbook: &Workbook, projection: &mut DocumentData);

    fn sync_merge_ranges(
        &self,
        workbook: &mut Workbook,
        projection: &DocumentData,
    ) -> Result<(), AppError>;

    fn validate_projection(
        &self,
        workbook: &Workbook,
        projection: &DocumentData,
    ) -> Result<(), AppError>;

    fn validate_projection_sheets(
        &self,
        workbook: &Workbook,
        projection: &DocumentData,
        sheet_indexes: &[usize],
    ) -> Result<(), AppError>;

    fn sync_sheet(&self, worksheet: &mut Worksheet, sheet: &DocumentSheet) -> Result<(), AppError>;

    fn write_cell(&self, worksheet: &mut Worksheet, row: u32, col: u32, value: &CellValue);

    fn column_width_from_pixels(&self, pixels: u32) -> f64;

    fn row_height_from_pixels(&self, pixels: u32) -> f64;
}
