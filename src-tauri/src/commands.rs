use std::path::Path;

use crate::error::AppError;
use crate::reader;
use crate::types::FileData;
use crate::writer;

#[tauri::command]
pub fn read_file(path: String) -> Result<FileData, AppError> {
    let path = Path::new(&path);
    reader::read_file(path)
}

#[tauri::command]
pub fn save_file(path: String, file_data: FileData) -> Result<(), AppError> {
    let path = Path::new(&path);
    writer::save_file(path, &file_data)
}

#[tauri::command]
pub fn get_default_save_path(file_name: String) -> String {
    if let Some(dot_pos) = file_name.rfind('.') {
        let name = &file_name[..dot_pos];
        format!("{}_edited.xlsx", name)
    } else {
        format!("{}_edited.xlsx", file_name)
    }
}
