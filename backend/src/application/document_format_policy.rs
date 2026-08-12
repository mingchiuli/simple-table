use crate::document_format::{default_spreadsheet_extension, extension_of};
use crate::error::AppError;
use crate::state::editor_state::EditorState;

const LOSSY_CSV_SAVE_REASON: &str = "Saving a non-CSV document as CSV would discard sheets, formulas, or formatting; export to CSV explicitly instead.";

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
    Ok(())
}
