mod adapters;
mod application;
mod commands;
mod document;
mod document_data;
mod document_format;
mod document_layout_policy;
mod document_resource_estimator;
mod domain;
mod editor_protocol;
mod error;
mod formula;
mod io;
mod ops;
mod projection_model;
mod protocol_projection;
mod recent;
mod resource_limits;
mod runtime;
mod state;
mod types;
mod utils;

#[cfg(target_os = "android")]
use commands::android::{
    discard_open_file_selection_android, discard_save_location_android, export_file_android,
    pick_open_file_android, pick_save_location_android, prepare_open_file_android,
    save_file_android,
};
#[cfg(any(target_os = "android", target_os = "ios"))]
use commands::check_update_mobile;
#[cfg(target_os = "ios")]
use commands::ios::{
    discard_open_file_selection_ios, discard_save_location_ios, export_file_ios,
    pick_open_file_ios, pick_save_location_ios, prepare_open_file_ios, save_file_ios,
};
use commands::{
    abort_prepared_document, add_column, add_recent_file_with_thumbnail, add_row, add_sheet,
    close_current_document, commit_prepared_document, delete_column, delete_row, delete_sheet,
    get_active_document, get_current_document_projection, get_document_capabilities,
    get_editor_state, get_mutation_result, get_native_save_plan, get_recent_files,
    get_sheet_region_projection, get_spreadsheet_format_options, prepare_new_file, redo,
    remove_recent_file, search, set_cell, set_cells, set_column_width, set_row_height, undo,
};
#[cfg(desktop)]
use commands::{
    acknowledge_open_target_desktop, claim_pending_open_target_desktop,
    discard_open_file_selection_desktop, discard_save_location_desktop, export_file_desktop,
    pick_open_file_desktop, pick_save_location_desktop, prepare_open_file_desktop,
    prepare_recent_file_desktop, release_open_target_desktop, save_file_desktop,
};

use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default()
        .manage(runtime::ApplicationRuntime::default())
        .manage(commands::CommandExecutionRuntime::default());

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            println!("new app instance opened with {argv:?}");
            let runtime = app.state::<runtime::ApplicationRuntime>();
            let mut enqueued = false;
            for target in argv.iter().skip(1) {
                enqueued |= runtime.platform_files().enqueue_open_target(target);
            }
            if enqueued {
                if let Err(e) = app.emit("deep-link-received", ()) {
                    eprintln!("Failed to emit deep link: {}", e);
                }
            }
        }));
    }

    // Desktop: updater + process for auto-update
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
        builder = builder.plugin(tauri_plugin_process::init());
    }

    #[cfg(desktop)]
    {
        builder = builder.setup(|app| {
            let platform_files = app
                .state::<runtime::ApplicationRuntime>()
                .platform_files()
                .clone();
            for arg in std::env::args().skip(1) {
                platform_files.enqueue_open_target(&arg);
            }

            use tauri_plugin_deep_link::DeepLinkExt;
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                for url in urls {
                    platform_files.enqueue_open_target(url.as_str());
                }
            }
            let app_handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                let mut enqueued = false;
                for url in event.urls() {
                    enqueued |= platform_files.enqueue_open_target(url.as_str());
                }
                if enqueued && let Err(error) = app_handle.emit("deep-link-received", ()) {
                    eprintln!("Failed to emit deep link: {error}");
                }
            });
            Ok(())
        });
    }

    #[cfg(mobile)]
    {
        builder = builder.plugin(tauri_plugin_fs::init());
        builder = builder.setup(|app| {
            let runtime = app.state::<runtime::ApplicationRuntime>();
            runtime
                .platform_files()
                .reconcile_transient_files(app.handle())?;
            Ok(())
        });
    }

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_os::init())
        .invoke_handler(tauri::generate_handler![
            #[cfg(desktop)]
            pick_open_file_desktop,
            #[cfg(desktop)]
            discard_open_file_selection_desktop,
            #[cfg(desktop)]
            claim_pending_open_target_desktop,
            #[cfg(desktop)]
            acknowledge_open_target_desktop,
            #[cfg(desktop)]
            release_open_target_desktop,
            #[cfg(desktop)]
            prepare_open_file_desktop,
            #[cfg(desktop)]
            prepare_recent_file_desktop,
            #[cfg(desktop)]
            pick_save_location_desktop,
            #[cfg(desktop)]
            discard_save_location_desktop,
            #[cfg(desktop)]
            save_file_desktop,
            #[cfg(desktop)]
            export_file_desktop,
            get_current_document_projection,
            get_sheet_region_projection,
            close_current_document,
            get_document_capabilities,
            get_native_save_plan,
            get_spreadsheet_format_options,
            prepare_new_file,
            commit_prepared_document,
            abort_prepared_document,
            get_active_document,
            get_mutation_result,
            undo,
            redo,
            set_cell,
            set_cells,
            add_row,
            delete_row,
            add_column,
            delete_column,
            set_column_width,
            set_row_height,
            add_sheet,
            delete_sheet,
            get_editor_state,
            search,
            get_recent_files,
            add_recent_file_with_thumbnail,
            remove_recent_file,
            // Android 专用命令
            #[cfg(target_os = "android")]
            pick_open_file_android,
            #[cfg(target_os = "android")]
            discard_open_file_selection_android,
            #[cfg(target_os = "android")]
            discard_save_location_android,
            #[cfg(target_os = "android")]
            prepare_open_file_android,
            #[cfg(target_os = "android")]
            save_file_android,
            #[cfg(target_os = "android")]
            export_file_android,
            #[cfg(target_os = "android")]
            pick_save_location_android,
            // iOS 专用命令
            #[cfg(target_os = "ios")]
            pick_open_file_ios,
            #[cfg(target_os = "ios")]
            discard_open_file_selection_ios,
            #[cfg(target_os = "ios")]
            discard_save_location_ios,
            #[cfg(target_os = "ios")]
            prepare_open_file_ios,
            #[cfg(target_os = "ios")]
            pick_save_location_ios,
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
