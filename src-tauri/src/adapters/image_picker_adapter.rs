use tauri::AppHandle;
use tauri_plugin_fs::FilePath;

use crate::error::AppError;

pub(crate) struct SelectedImageFile {
    pub file_name: String,
    pub bytes: Vec<u8>,
}

pub(crate) fn pick_image_file(app: &AppHandle) -> Result<Option<SelectedImageFile>, AppError> {
    use tauri_plugin_dialog::{DialogExt, PickerMode};

    let Some(path) = app
        .dialog()
        .file()
        .add_filter("Image", &["png", "jpg", "jpeg"])
        .set_picker_mode(PickerMode::Document)
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let file_name = selected_file_name(&path).unwrap_or_else(|| "image.png".to_string());
    let bytes = read_selected_image(app, path)?;
    Ok(Some(SelectedImageFile { file_name, bytes }))
}

fn selected_file_name(path: &FilePath) -> Option<String> {
    match path {
        FilePath::Path(path) => path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string),
        FilePath::Url(url) => url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .filter(|name| !name.is_empty())
            .map(str::to_string),
    }
}

#[cfg(desktop)]
fn read_selected_image(_app: &AppHandle, path: FilePath) -> Result<Vec<u8>, AppError> {
    let path = path
        .into_path()
        .map_err(|error| AppError::ReadError(error.to_string()))?;
    std::fs::read(path).map_err(|error| AppError::ReadError(error.to_string()))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn read_selected_image(app: &AppHandle, path: FilePath) -> Result<Vec<u8>, AppError> {
    crate::io::platform::mobile::read_with_official_fs(app, path)
}
