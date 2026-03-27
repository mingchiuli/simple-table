mod commands;
mod error;
mod io;
mod ops;
mod state;
mod types;

use commands::{
    add_column, add_row, add_sheet, delete_column, delete_row, delete_sheet,
    generate_file_bytes, get_default_save_path,
    get_editor_state, get_file_data, init_file, read_file, read_file_bytes,
    redo, save_file, search, set_cell, sort_column, undo,
};
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;

struct PendingDeepLink(Mutex<Option<String>>);

#[tauri::command]
fn get_pending_deep_link(app: tauri::AppHandle) -> Option<String> {
    app.state::<PendingDeepLink>().0.lock().expect("PendingDeepLink lock poisoned").take()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            println!("new app instance opened with {argv:?}");
            // argv[0] is the app path, argv[1+] are the arguments
            if argv.len() > 1 {
                let url = &argv[1];
                if url.starts_with("simpletable://") {
                    if let Err(e) = app.emit("deep-link-received", url.clone()) {
                        eprintln!("Failed to emit deep link: {}", e);
                    }
                }
            }
        }));
    }

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_deep_link::init())
        .manage(PendingDeepLink(Mutex::new(None)))
        .setup(|app| {
            // Register on_open_url for macOS file associations (double-click in Finder)
            #[cfg(desktop)]
            {
                let handle = app.handle().clone();
                app.deep_link().on_open_url(move |event| {
                    for url in event.urls() {
                        let url_str = url.to_string();
                        println!("Opened via file association: {}", url_str);

                        // Store URL in app state for frontend to retrieve
                        if let Some(state) = handle.try_state::<PendingDeepLink>() {
                            let mut pending = state.0.lock().expect("PendingDeepLink lock poisoned");
                            *pending = Some(url_str);
                        }
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            read_file,
            read_file_bytes,
            save_file,
            generate_file_bytes,
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
            sort_column,
            get_editor_state,
            search,
            get_pending_deep_link
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
