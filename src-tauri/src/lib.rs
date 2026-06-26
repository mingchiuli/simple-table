mod commands;
mod error;
mod formula;
mod io;
mod ops;
mod recent;
mod state;
mod types;
mod update;
mod utils;

#[cfg(target_os = "android")]
use commands::android::{
    export_file_android, pick_file_android, pick_save_location_android, read_file_android,
    save_file_android,
};
#[cfg(any(target_os = "android", target_os = "ios"))]
use commands::check_update_mobile;
#[cfg(target_os = "ios")]
use commands::ios::{
    create_private_file_ios, export_file_ios, pick_file_ios, read_file_ios, save_file_ios,
};
use commands::{
    add_column, add_recent_file_with_thumbnail, add_row, add_sheet, check_file_exists,
    delete_column, delete_row, delete_sheet, generate_file_bytes, generate_thumbnail_bytes,
    get_editor_state, get_file_size, get_recent_files, init_file, mark_file_saved, redo,
    remove_recent_file, search, set_cell, set_column_width, set_row_height, sort_column, undo,
    update_recent_file_path,
};
#[cfg(desktop)]
use commands::{read_file_desktop, save_file_desktop};

use tauri::Emitter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            println!("new app instance opened with {argv:?}");
            // File path is passed as argv[1], emit event for frontend to handle
            if argv.len() > 1
                && let Err(e) = app.emit("deep-link-received", argv[1].clone())
            {
                eprintln!("Failed to emit deep link: {}", e);
            }
        }));
    }

    // Desktop: updater + process for auto-update
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
        builder = builder.plugin(tauri_plugin_process::init());
    }

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_os::init())
        .invoke_handler(tauri::generate_handler![
            #[cfg(desktop)]
            read_file_desktop,
            #[cfg(desktop)]
            save_file_desktop,
            generate_file_bytes,
            generate_thumbnail_bytes,
            init_file,
            undo,
            redo,
            set_cell,
            add_row,
            delete_row,
            add_column,
            delete_column,
            set_column_width,
            set_row_height,
            add_sheet,
            delete_sheet,
            sort_column,
            get_editor_state,
            mark_file_saved,
            search,
            get_recent_files,
            add_recent_file_with_thumbnail,
            remove_recent_file,
            check_file_exists,
            get_file_size,
            update_recent_file_path,
            // Android 专用命令
            #[cfg(target_os = "android")]
            pick_file_android,
            #[cfg(target_os = "android")]
            read_file_android,
            #[cfg(target_os = "android")]
            save_file_android,
            #[cfg(target_os = "android")]
            export_file_android,
            #[cfg(target_os = "android")]
            pick_save_location_android,
            // iOS 专用命令
            #[cfg(target_os = "ios")]
            pick_file_ios,
            #[cfg(target_os = "ios")]
            read_file_ios,
            #[cfg(target_os = "ios")]
            create_private_file_ios,
            #[cfg(target_os = "ios")]
            save_file_ios,
            #[cfg(target_os = "ios")]
            export_file_ios,
            // Mobile: 检查更新命令
            #[cfg(any(target_os = "android", target_os = "ios"))]
            check_update_mobile
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
