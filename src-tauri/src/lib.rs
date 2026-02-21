mod commands;
mod error;
mod reader;
mod types;
mod writer;

use commands::{get_default_save_path, read_file, save_file};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![read_file, save_file, get_default_save_path])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
