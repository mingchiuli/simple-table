mod editor_state;
mod error;
mod index_ops;
mod reader;
mod types;
mod writer;

// Command modules (depends on index_ops)
mod state;
mod editor_ops;
mod cell_ops;
mod file_ops;
mod search_ops;
mod commands;

use commands::{get_default_save_path, read_file, save_file, init_file, get_file_data, undo, redo, set_cell, add_row, delete_row, add_column, delete_column, add_sheet, delete_sheet, get_editor_state, search};

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
            init_file,
            get_file_data,
            undo,
            redo,
            set_cell,
            add_row,
            delete_row,
            add_column,
            delete_column,
            add_sheet,
            delete_sheet,
            get_editor_state,
            search
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
