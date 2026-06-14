use super::mobile::{
    PickFileResult, PickedFileInfo, extension_from_name, mobile_dir, unique_import_path,
    write_path_with_official_fs,
};
use crate::error::AppError;
use crate::io::document;
use std::path::Path;
use std::str;
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
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .filter(|segment| !segment.is_empty())
            .map(|segment| segment.to_string())
            .unwrap_or_else(|| "imported.xlsx".to_string()),
    }
}

fn supported_extension_from_name(file_name: &str) -> Option<String> {
    let ext = Path::new(file_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase())?;
    match ext.as_str() {
        "xlsx" | "xls" | "csv" | "ods" => Some(ext),
        _ => None,
    }
}

fn extension_for_import(file_name: &str, bytes: &[u8]) -> String {
    if let Some(ext) = supported_extension_from_name(file_name) {
        return ext;
    }

    if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        return "xls".to_string();
    }

    if bytes.starts_with(b"PK") {
        return "xlsx".to_string();
    }

    if str::from_utf8(bytes).is_ok() {
        return "csv".to_string();
    }

    "xlsx".to_string()
}

fn normalize_display_name(file_name: String, extension: &str) -> String {
    if supported_extension_from_name(&file_name).is_some() {
        file_name
    } else {
        format!("imported.{}", extension)
    }
}

pub fn pick_file(app: &AppHandle) -> Result<Option<PickFileResult>, AppError> {
    use tauri_plugin_dialog::{DialogExt, PickerMode};
    use tauri_plugin_fs::FsExt;

    let source = match app
        .dialog()
        .file()
        .add_filter(
            "Spreadsheet",
            &[
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                "application/vnd.ms-excel",
                "text/csv",
                "application/vnd.oasis.opendocument.spreadsheet",
                "*/*",
            ],
        )
        .set_picker_mode(PickerMode::Document)
        .blocking_pick_file()
    {
        Some(path) => path,
        None => return Ok(None),
    };

    let original_path = source.to_string();
    let raw_file_name = display_name_from_path(&source);
    let bytes = app
        .fs()
        .read(source)
        .map_err(|e| AppError::ReadError(format!("Failed to read selected file: {}", e)))?;

    let extension = extension_for_import(&raw_file_name, &bytes);
    let file_name = normalize_display_name(raw_file_name, &extension);
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

pub fn pick_save_location(app: &AppHandle, default_name: &str) -> Result<String, AppError> {
    let stem = Path::new(default_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("untitled");
    let path = mobile_dir(app)?.join(format!(
        "{}-{}.{}",
        stem,
        uuid::Uuid::new_v4(),
        extension_from_name(default_name)
    ));
    Ok(path.to_string_lossy().to_string())
}
