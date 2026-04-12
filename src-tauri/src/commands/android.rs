#[cfg(target_os = "android")]
use crate::error::AppError;
#[cfg(target_os = "android")]
use serde::{Deserialize, Serialize};
#[cfg(target_os = "android")]
use tauri::AppHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg(target_os = "android")]
pub struct PickedFile {
    pub path: String,
    pub file_name: String,
    pub bytes: Vec<u8>,
}

// ==================== Android Commands ====================

#[cfg(target_os = "android")]
#[tauri::command]
pub async fn pick_file_android(app: AppHandle) -> Result<Option<PickedFile>, AppError> {
    use tauri_plugin_android_fs::{AndroidFsExt, FileUri};

    let api = app.android_fs();

    // 打开文件选择器
    let uri = match api
        .file_picker()
        .pick_file(None, &["*/*"], false)
        .map_err(|e| AppError::ReadError(e.to_string()))?
    {
        Some(uri) => uri,
        None => return Ok(None),
    };

    // 持久化 URI 权限（关键步骤，使重启后仍可访问）
    api.file_picker()
        .persist_uri_permission(&uri)
        .map_err(|e| AppError::ReadError(format!("Failed to persist permission: {}", e)))?;

    // 获取文件名和内容
    let file_name = api
        .get_name(&uri)
        .map_err(|e| AppError::ReadError(e.to_string()))?;

    let bytes = api
        .read(&uri)
        .map_err(|e| AppError::ReadError(e.to_string()))?;

    Ok(Some(PickedFile {
        path: uri.uri.clone(),
        file_name,
        bytes,
    }))
}

#[cfg(target_os = "android")]
#[tauri::command]
pub async fn read_file_android(app: AppHandle, uri: String) -> Result<Vec<u8>, AppError> {
    use tauri_plugin_android_fs::{AndroidFsExt, FileUri};

    let api = app.android_fs();
    api.read(&FileUri {
        uri,
        document_top_tree_uri: None,
    })
    .map_err(|e| AppError::ReadError(e.to_string()))
}

#[cfg(target_os = "android")]
#[tauri::command]
pub async fn save_file_android(app: AppHandle, uri: String, bytes: Vec<u8>) -> Result<(), AppError> {
    use tauri_plugin_android_fs::{AndroidFsExt, FileUri};

    let api = app.android_fs();
    api.write(
        &FileUri {
            uri,
            document_top_tree_uri: None,
        },
        bytes,
    )
    .map_err(|e| AppError::WriteError(e.to_string()))
}

#[cfg(target_os = "android")]
#[tauri::command]
pub async fn pick_save_location_android(app: AppHandle, default_name: String) -> Result<Option<String>, AppError> {
    use tauri_plugin_android_fs::{AndroidFsExt};

    let api = app.android_fs();

    let uri = api
        .file_picker()
        .save_file(None, default_name, None, false)
        .map_err(|e| AppError::ReadError(e.to_string()))?;

    match uri {
        Some(uri) => {
            // 持久化写入权限
            api.file_picker()
                .persist_uri_permission(&uri)
                .map_err(|e| AppError::ReadError(format!("Failed to persist permission: {}", e)))?;
            Ok(Some(uri.uri.clone()))
        }
        None => Ok(None),
    }
}

