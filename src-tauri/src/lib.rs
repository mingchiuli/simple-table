mod commands;
mod error;
mod io;
mod ops;
mod recent;
mod state;
mod types;
mod utils;

use commands::{
    add_column, add_recent_file_with_thumbnail, add_row, add_sheet,
    check_file_exists, delete_column, delete_row, delete_sheet,
    generate_file_bytes, get_editor_state, get_recent_files, init_file,
    read_file_bytes, redo, remove_recent_file, search, set_cell, sort_column, undo,
    update_recent_file_path,
};
#[cfg(target_os = "android")]
use commands::android::{pick_file_android, pick_save_location_android, read_file_android, save_file_android};
#[cfg(target_os = "ios")]
use commands::ios::{create_private_file_ios, export_file_ios, pick_file_ios, save_file_ios, silent_export_file_ios};
use std::sync::Mutex;
use tauri::{Emitter, Manager};
#[cfg(desktop)]
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
            // File path is passed as argv[1], emit event for frontend to handle
            if argv.len() > 1 {
                if let Err(e) = app.emit("deep-link-received", argv[1].clone()) {
                    eprintln!("Failed to emit deep link: {}", e);
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
            #[cfg(target_os = "android")]
            pick_file_android,
            #[cfg(target_os = "android")]
            read_file_android,
            #[cfg(target_os = "android")]
            save_file_android,
            #[cfg(target_os = "android")]
            pick_save_location_android,
            // iOS 专用命令
            #[cfg(target_os = "ios")]
            pick_file_ios,
            #[cfg(target_os = "ios")]
            create_private_file_ios,
            #[cfg(target_os = "ios")]
            save_file_ios,
            #[cfg(target_os = "ios")]
            export_file_ios,
            #[cfg(target_os = "ios")]
            silent_export_file_ios
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
