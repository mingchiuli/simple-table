use super::mobile::{
    PickFileResult, PickedFileInfo, mobile_dir, unique_import_path, write_path_with_official_fs,
};
use crate::error::AppError;
use crate::io::document;
use crate::io::file_format::{
    default_spreadsheet_file_name, file_name_from_path_like, file_stem_from_path_like,
    import_extension_from_name_or_bytes, normalized_import_file_name,
    supported_extension_or_default,
};
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
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .filter(|segment| !segment.is_empty())
            .map(|segment| {
                file_name_from_path_like(segment, &default_spreadsheet_file_name("imported"))
            })
            .unwrap_or_else(|| default_spreadsheet_file_name("imported")),
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
                "text/csv",
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

    let extension = import_extension_from_name_or_bytes(&raw_file_name, &bytes)
        .ok_or(AppError::UnsupportedFormat)?;
    let file_name = normalized_import_file_name(&raw_file_name, &extension);
    let sandbox_path = unique_import_path(app, &file_name)?;
    write_path_with_official_fs(app, sandbox_path.clone(), &bytes)?;

    let path = sandbox_path.to_string_lossy().to_string();
    let document = document::open_from_bytes(path.clone(), bytes, Some(file_name.clone()))?;

    Ok(Some(PickFileResult {
        document,
        info: PickedFileInfo {
            path,
            original_path,
            file_name,
        },
    }))
}

pub fn pick_save_location(app: &AppHandle, default_name: &str) -> Result<String, AppError> {
    let stem = file_stem_from_path_like(default_name, "untitled");
    let path = mobile_dir(app)?.join(format!(
        "{}-{}.{}",
        stem,
        uuid::Uuid::new_v4(),
        supported_extension_or_default(default_name)
    ));
    Ok(path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_extension_requires_supported_extension_for_zip_files() {
        assert_eq!(
            import_extension_from_name_or_bytes("book.xlsx", b"PK\x03\x04"),
            Some("xlsx".to_string())
        );
        assert_eq!(
            import_extension_from_name_or_bytes("data.csv", b"a,b"),
            Some("csv".to_string())
        );
        assert_eq!(
            import_extension_from_name_or_bytes("unknown", b"PK\x03\x04"),
            Some("xlsx".to_string())
        );
        assert_eq!(
            import_extension_from_name_or_bytes("unsupported.bin", b"PK\x03\x04"),
            None
        );
    }
}
