use super::mobile::{
    self, MobileFileRuntime, read_with_official_fs, unique_import_path, write_path_with_official_fs,
};
use crate::document_format::{
    SUPPORTED_SPREADSHEET_EXTENSIONS, default_spreadsheet_file_name, file_name_from_path_like,
    import_extension_from_name_or_bytes, normalized_import_file_name,
};
use crate::error::AppError;
use crate::io::open_file_input::OpenFileSelection;
use crate::io::transient_files::TransientFilePurpose;
use tauri::AppHandle;
use tauri_plugin_fs::FilePath;

fn display_name_from_path(path: &FilePath) -> String {
    match path {
        FilePath::Path(p) => p
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_string())
            .unwrap_or_else(|| default_spreadsheet_file_name("imported")),
        FilePath::Url(url) => url
            .to_file_path()
            .ok()
            .and_then(|p| {
                p.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.to_string())
            })
            .or_else(|| {
                url.path_segments()
                    .and_then(|mut segments| segments.next_back())
                    .filter(|segment| !segment.is_empty())
                    .map(|segment| {
                        file_name_from_path_like(
                            segment,
                            &default_spreadsheet_file_name("imported"),
                        )
                    })
            })
            .unwrap_or_else(|| default_spreadsheet_file_name("imported")),
    }
}

pub fn pick_file_info(
    runtime: &MobileFileRuntime,
    app: &AppHandle,
) -> Result<Option<OpenFileSelection>, AppError> {
    use tauri_plugin_dialog::{DialogExt, FileAccessMode, PickerMode};

    let source = match app
        .dialog()
        .file()
        .add_filter("Spreadsheet", SUPPORTED_SPREADSHEET_EXTENSIONS)
        .set_picker_mode(PickerMode::Document)
        .set_file_access_mode(FileAccessMode::Copy)
        .blocking_pick_file()
    {
        Some(path) => path,
        None => return Ok(None),
    };

    let original_path = source.to_string();
    let raw_file_name = display_name_from_path(&source);
    let bytes = read_with_official_fs(app, source)?;

    let extension = import_extension_from_name_or_bytes(&raw_file_name, &bytes)
        .ok_or(AppError::UnsupportedFormat)?;
    let file_name = normalized_import_file_name(&raw_file_name, &extension);
    let sandbox_path = unique_import_path(runtime, app, &file_name)?;
    write_path_with_official_fs(app, sandbox_path.clone(), &bytes)?;
    mobile::register_created_transient_path(
        runtime,
        app,
        &sandbox_path,
        TransientFilePurpose::OpenSelection,
    )?;

    let path = sandbox_path.to_string_lossy().to_string();

    Ok(Some(OpenFileSelection {
        path,
        original_path,
        file_name,
    }))
}
