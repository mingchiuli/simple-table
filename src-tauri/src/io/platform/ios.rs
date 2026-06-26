use super::mobile::{
    self, PickFileResult, PickedFileInfo, unique_import_path, write_path_with_official_fs,
};
use crate::error::AppError;
use crate::io::document;
use tauri::AppHandle;
use tauri_plugin_fs::FilePath;

fn display_name_from_path(path: &FilePath) -> String {
    match path {
        FilePath::Path(p) => p
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_string())
            .unwrap_or_else(|| "imported.xlsx".to_string()),
        FilePath::Url(url) => url
            .to_file_path()
            .ok()
            .and_then(|p| {
                p.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.to_string())
            })
            .unwrap_or_else(|| "imported.xlsx".to_string()),
    }
}

pub fn pick_file(app: &AppHandle) -> Result<Option<PickFileResult>, AppError> {
    use tauri_plugin_dialog::{DialogExt, FileAccessMode, PickerMode};
    use tauri_plugin_fs::FsExt;

    let source = match app
        .dialog()
        .file()
        .add_filter("Spreadsheet", &["xlsx", "xlsm", "csv"])
        .set_picker_mode(PickerMode::Document)
        .set_file_access_mode(FileAccessMode::Copy)
        .blocking_pick_file()
    {
        Some(path) => path,
        None => return Ok(None),
    };

    let original_path = source.to_string();
    let file_name = display_name_from_path(&source);
    let bytes = app
        .fs()
        .read(source)
        .map_err(|e| AppError::ReadError(format!("Failed to read selected file: {}", e)))?;

    let sandbox_path = unique_import_path(app, &file_name)?;
    write_path_with_official_fs(app, sandbox_path.clone(), &bytes)?;

    let path = sandbox_path.to_string_lossy().to_string();
    let file_data =
        document::open_from_bytes(path.clone(), bytes.clone(), Some(file_name.clone()))?;

    Ok(Some(PickFileResult {
        file_data,
        info: PickedFileInfo {
            path,
            original_path,
            file_name,
        },
        bytes,
    }))
}

pub fn export_file(
    app: &AppHandle,
    source_path: &str,
    default_name: &str,
) -> Result<Option<String>, AppError> {
    mobile::export_file(app, source_path, default_name)
}
