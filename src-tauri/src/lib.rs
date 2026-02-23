mod command;
mod error;
mod reader;
mod types;
mod writer;

// Command modules
mod state;
mod file_ops;
mod editor_ops;
mod cell_ops;
mod search_ops;
mod tauri_commands;

use tauri_commands::{get_default_save_path, read_file, save_file, undo, redo, set_cell, add_row, delete_row, add_column, delete_column, get_editor_state, search};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            read_file,
            save_file,
            get_default_save_path,
            undo,
            redo,
            set_cell,
            add_row,
            delete_row,
            add_column,
            delete_column,
            get_editor_state,
            search
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
