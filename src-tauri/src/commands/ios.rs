#[cfg(target_os = "ios")]
use std::fs;
#[cfg(target_os = "ios")]
use std::path::PathBuf;
#[cfg(target_os = "ios")]
use serde::{Deserialize, Serialize};
#[cfg(target_os = "ios")]
use tauri::AppHandle;
#[cfg(target_os = "ios")]
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg(target_os = "ios")]
pub struct PickedFile {
    /// 私有目录路径（实际文件存储位置）
    pub path: String,
    /// 原始文件路径（用于显示）
    pub original_path: String,
    /// 文件名
    pub file_name: String,
}

// ==================== iOS Commands ====================

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn pick_file_ios(app: AppHandle) -> Result<PickedFile, AppError> {
    use tauri::Manager;
    use tauri_plugin_dialog::DialogExt;

    // 1. 打开文件选择器
    let file_path = app
        .dialog()
        .file()
        .add_filter("Spreadsheet", &["xlsx", "xls", "csv", "ods"])
        .blocking_pick_file()
        .ok_or_else(|| AppError::ReadError("No file selected".to_string()))?;

    let source_path: PathBuf = file_path.into_path()
        .map_err(|e| AppError::ReadError(format!("Failed to get path: {}", e)))?;

    // 获取文件名
    let file_name = source_path
        .file_name()
        .and_then(|n: &std::ffi::OsStr| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    // 2. 获取 App Data 目录（私有，用户不可见）
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::ReadError(format!("Failed to get app data dir: {}", e)))?;

    // 创建私有存储子目录
    let private_dir = data_dir.join("SimpleTablePrivate");
    fs::create_dir_all(&private_dir)
        .map_err(|e| AppError::WriteError(format!("Failed to create private dir: {}", e)))?;

    // 3. 生成唯一文件名（使用 UUID 防止冲突）
    let uuid = uuid::Uuid::new_v4().to_string();
    let extension = PathBuf::from(&file_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string();
    let private_file_name = format!("{}.{}", uuid, extension);
    let private_path = private_dir.join(&private_file_name);

    // 4. 复制文件到私有目录
    fs::copy(&source_path, &private_path)
        .map_err(|e| AppError::WriteError(format!("Failed to copy file: {}", e)))?;

    Ok(PickedFile {
        path: private_path.to_string_lossy().to_string(),
        original_path: source_path.to_string_lossy().to_string(),
        file_name,
    })
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn create_private_file_ios(app: AppHandle, file_name: String) -> Result<PickedFile, AppError> {
    use tauri::Manager;

    // 获取 App Data 目录
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::ReadError(format!("Failed to get app data dir: {}", e)))?;

    // 创建私有存储子目录
    let private_dir = data_dir.join("SimpleTablePrivate");
    fs::create_dir_all(&private_dir)
        .map_err(|e| AppError::WriteError(format!("Failed to create private dir: {}", e)))?;

    // 生成唯一文件名
    let uuid = uuid::Uuid::new_v4().to_string();
    let extension = PathBuf::from(&file_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("xlsx")
        .to_string();
    let private_file_name = format!("{}.{}", uuid, extension);
    let private_path = private_dir.join(&private_file_name);

    // 创建空文件
    fs::write(&private_path, [])
        .map_err(|e| AppError::WriteError(format!("Failed to create file: {}", e)))?;

    Ok(PickedFile {
        path: private_path.to_string_lossy().to_string(),
        original_path: String::new(),
        file_name,
    })
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn save_file_ios(_app: AppHandle, path: String, bytes: Vec<u8>) -> Result<(), AppError> {
    // 写入私有目录中的文件
    fs::write(&path, bytes)
        .map_err(|e| AppError::WriteError(format!("Failed to save file: {}", e)))?;
    Ok(())
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn export_file_ios(
    app: AppHandle,
    source_path: String,
    default_name: String,
) -> Result<Option<String>, AppError> {
    use tauri_plugin_dialog::DialogExt;

    // 打开保存对话框让用户选择导出位置
    let save_path = app
        .dialog()
        .file()
        .add_filter("Spreadsheet", &["xlsx", "xls", "csv", "ods"])
        .set_file_name(&default_name)
        .blocking_save_file();

    match save_path {
        Some(dest_path) => {
            let dest_path: PathBuf = dest_path.into_path()
                .map_err(|e| AppError::WriteError(format!("Failed to get path: {}", e)))?;
            // 复制私有目录文件到用户选择的位置
            fs::copy(&source_path, &dest_path)
                .map_err(|e| AppError::WriteError(format!("Failed to export file: {}", e)))?;
            Ok(Some(dest_path.to_string_lossy().to_string()))
        }
        None => Ok(None),
    }
}

