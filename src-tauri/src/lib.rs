mod commands;
mod error;
mod io;
mod ops;
mod state;
mod types;

use commands::{
    add_column, add_row, add_sheet, delete_column, delete_row, delete_sheet, get_default_save_path,
    get_editor_state, get_file_data, init_file, read_file, redo, save_file, search, set_cell, undo,
};

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
