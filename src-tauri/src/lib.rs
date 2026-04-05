mod commands;
mod error;
mod io;
mod mobile;
mod ops;
mod state;
mod types;

use commands::{
    add_column, add_recent_file_with_thumbnail, add_row, add_sheet,
    check_file_exists, delete_column, delete_row, delete_sheet,
    generate_file_bytes, get_editor_state, get_recent_files, init_file,
    read_file_bytes, redo, remove_recent_file, search, set_cell, sort_column, undo,
    update_recent_file_path,
};
use mobile::{pick_file_android, pick_save_location_android, read_file_android, save_file_android};
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

    // Android 专用文件系统插件（支持持久化 URI 权限）
    #[cfg(target_os = "android")]
    {
        builder = builder.plugin(tauri_plugin_android_fs::init());
    }

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_os::init())
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
            read_file_bytes,
            generate_file_bytes,
            init_file,
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
            get_pending_deep_link,
            get_recent_files,
            add_recent_file_with_thumbnail,
            remove_recent_file,
            check_file_exists,
            update_recent_file_path,
            // Android 专用命令
            pick_file_android,
            read_file_android,
            save_file_android,
            pick_save_location_android
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
