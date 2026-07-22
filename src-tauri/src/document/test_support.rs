use std::sync::Arc;

use crate::document::backing::workbook_port::WorkbookBackingPort;
use crate::document_data::DocumentData;
use crate::error::AppError;
use crate::io::codec::writer;
use crate::io::projection_codec::WorkbookProjectionCodec;
use umya_spreadsheet::Workbook;

pub(crate) use crate::io::codec::reader::read_file_with_workbook_from_bytes;

pub(crate) fn workbook_backing_port() -> Arc<dyn WorkbookBackingPort> {
    Arc::new(WorkbookProjectionCodec)
}

pub(crate) fn generate_file_bytes_for_target(
    workbook: Option<&Workbook>,
    projection: &DocumentData,
    target_path_or_name: &str,
) -> Result<(String, Vec<u8>), AppError> {
    if workbook.is_some()
        && crate::document_format::SpreadsheetFileFormat::from_path_or_default(target_path_or_name)
            .is_some_and(crate::document_format::SpreadsheetFileFormat::is_xlsx)
    {
        return writer::generate_excel_bytes_from_workbook_for_target(
            workbook.expect("checked workbook"),
            target_path_or_name,
        );
    }
    writer::generate_file_bytes_for_target(projection, target_path_or_name)
}
