use crate::document_format::{SpreadsheetFileFormat, default_spreadsheet_extension, extension_of};
use crate::error::AppError;
use crate::state::editor_state::EditorState;

const LOSSY_CSV_SAVE_REASON: &str = "Saving a non-CSV document as CSV would discard sheets, formulas, or formatting; export to CSV explicitly instead.";
const EXCEL_FORMAT_CHANGE_REASON: &str =
    "Changing between XLSX and XLSM during save is not supported; keep the source extension.";

pub(crate) fn ensure_native_save_target_allowed(
    editor_state: &EditorState,
    target_path_or_name: &str,
) -> Result<(), AppError> {
    let target_extension = extension_of(target_path_or_name)
        .unwrap_or_else(|| default_spreadsheet_extension().to_string());
    if target_extension == "csv" && !editor_state.is_csv_backed() {
        return Err(AppError::DocumentStateInvalid(
            LOSSY_CSV_SAVE_REASON.to_string(),
        ));
    }
    let source_format = extension_of(&editor_state.file_data().file_name)
        .or_else(|| extension_of(&editor_state.file_data().path));
    let source_format = source_format
        .as_deref()
        .and_then(SpreadsheetFileFormat::from_extension);
    let target_format = SpreadsheetFileFormat::from_extension(&target_extension);
    if target_format == Some(SpreadsheetFileFormat::Xlsm)
        && source_format != Some(SpreadsheetFileFormat::Xlsm)
    {
        return Err(AppError::DocumentStateInvalid(
            EXCEL_FORMAT_CHANGE_REASON.to_string(),
        ));
    }
    if source_format.is_some_and(SpreadsheetFileFormat::is_excel)
        && target_format.is_some_and(SpreadsheetFileFormat::is_excel)
        && source_format != target_format
    {
        return Err(AppError::DocumentStateInvalid(
            EXCEL_FORMAT_CHANGE_REASON.to_string(),
        ));
    }
    Ok(())
}
