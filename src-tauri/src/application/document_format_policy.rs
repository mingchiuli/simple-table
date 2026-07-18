use crate::error::AppError;
use crate::io::file_format::{
    default_spreadsheet_extension, export_extensions, extension_of, spreadsheet_format_options,
    supported_extension_from_name,
};
use crate::state::editor_state::EditorState;
use crate::types::{
    DocumentCapabilities, NativeSavePlan, SpreadsheetFormatOptions, WorkbookCapabilities,
};

const LOSSY_CSV_SAVE_REASON: &str = "Saving a non-CSV document as CSV would discard sheets, formulas, or formatting; use Export instead.";

pub(crate) fn document_capabilities(editor_state: &EditorState) -> DocumentCapabilities {
    let file = editor_state.file_data();
    let current_path = (!file.path.is_empty()).then_some(file.path.as_str());
    capabilities_for_source(
        file.file_name.as_str(),
        current_path,
        editor_state.capabilities(),
    )
}

pub(crate) fn native_save_plan(
    editor_state: &EditorState,
    target_path_or_name: &str,
) -> NativeSavePlan {
    let source_format =
        document_format(target_path_or_name).unwrap_or_else(default_extension_string);
    let native_extension = native_save_extension(target_path_or_name);
    let native_save_allowed = native_extension.is_some();
    let export_extension =
        export_extension(target_path_or_name).unwrap_or_else(|| source_format.clone());
    let mut workbook = editor_state.capabilities();
    workbook.save.can_native_save = native_save_allowed && workbook.save.can_native_save;
    if let Some(reason) = native_save_target_block_reason(editor_state, target_path_or_name) {
        workbook.save.can_native_save = false;
        if !workbook
            .save
            .blocked_save_reasons
            .iter()
            .any(|item| item == reason)
        {
            workbook.save.blocked_save_reasons.push(reason.to_string());
        }
    }
    let capabilities = DocumentCapabilities {
        source_format: source_format.clone(),
        can_save_original: native_save_allowed,
        native_save_format: native_extension.clone(),
        export_formats: export_formats_for(&source_format),
        requires_save_as_for_native_save: native_extension.is_none(),
        native_save_extension: native_extension.clone(),
        export_extension,
        workbook,
    };
    let blocked_reason = native_save_blocked_reason(&capabilities);

    NativeSavePlan {
        can_save: blocked_reason.is_none(),
        requires_save_as: capabilities.requires_save_as_for_native_save,
        native_save_extension: native_extension.clone(),
        default_extension: native_extension.unwrap_or_else(default_extension_string),
        blocked_reason,
        capabilities,
    }
}

pub(crate) fn ensure_native_save_target_allowed(
    editor_state: &EditorState,
    target_path_or_name: &str,
) -> Result<(), AppError> {
    if let Some(reason) = native_save_target_block_reason(editor_state, target_path_or_name) {
        return Err(AppError::DocumentStateInvalid(reason.to_string()));
    }
    Ok(())
}

pub(crate) fn format_options() -> SpreadsheetFormatOptions {
    spreadsheet_format_options()
}

pub(crate) fn capabilities_for_source(
    file_name: &str,
    current_path: Option<&str>,
    mut workbook: WorkbookCapabilities,
) -> DocumentCapabilities {
    let source_name = current_path.unwrap_or(file_name);
    let source_format = document_format(source_name)
        .or_else(|| document_format(file_name))
        .unwrap_or_else(default_extension_string);
    let native_extension = native_save_extension(source_name);
    let native_save_allowed = native_extension.is_some();
    workbook.save.can_native_save = native_save_allowed && workbook.save.can_native_save;
    DocumentCapabilities {
        source_format: source_format.clone(),
        can_save_original: native_save_allowed,
        native_save_format: native_extension.clone(),
        export_formats: export_formats_for(&source_format),
        requires_save_as_for_native_save: native_extension.is_none(),
        native_save_extension: native_extension,
        export_extension: export_extension(file_name).unwrap_or(source_format),
        workbook,
    }
}

fn native_save_target_block_reason(
    editor_state: &EditorState,
    target_path_or_name: &str,
) -> Option<&'static str> {
    let target_extension =
        extension_of(target_path_or_name).unwrap_or_else(default_extension_string);
    (target_extension == "csv" && !editor_state.is_csv_backed()).then_some(LOSSY_CSV_SAVE_REASON)
}

fn native_save_blocked_reason(capabilities: &DocumentCapabilities) -> Option<String> {
    if capabilities.native_save_extension.is_none() {
        return Some("Native save is only supported as .xlsx or .csv.".to_string());
    }
    if !capabilities.workbook.save.can_native_save {
        return Some(first_reason(
            [
                &capabilities.workbook.save.blocked_save_reasons,
                &capabilities.workbook.structure.blocked_structure_reasons,
                &capabilities
                    .workbook
                    .structure
                    .blocked_sheet_structure_reasons,
                &capabilities.workbook.save.detected_features,
            ],
            "Workbook cannot be saved in its current state.",
        ));
    }
    None
}

fn first_reason<const N: usize>(reason_groups: [&Vec<String>; N], fallback: &str) -> String {
    reason_groups
        .into_iter()
        .flat_map(|reasons| reasons.iter())
        .next()
        .cloned()
        .unwrap_or_else(|| fallback.to_string())
}

fn native_save_extension(file_name: &str) -> Option<String> {
    if extension_of(file_name).is_none() {
        Some(default_extension_string())
    } else {
        supported_extension_from_name(file_name)
    }
}

fn export_extension(file_name: &str) -> Option<String> {
    native_save_extension(file_name)
}

fn document_format(file_name: &str) -> Option<String> {
    export_extension(file_name)
}

fn export_formats_for(_source_format: &str) -> Vec<String> {
    export_extensions()
}

fn default_extension_string() -> String {
    default_spreadsheet_extension().to_string()
}
