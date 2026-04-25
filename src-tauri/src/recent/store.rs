use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

use crate::error::AppError;

use super::types::RecentFile;

const STORE_FILE: &str = "recent-files.json";
const STORE_KEY: &str = "recent_files";
const MAX_RECENT: usize = 10;

pub struct RecentStore;

impl RecentStore {
    pub fn get_all(app: &AppHandle) -> Vec<RecentFile> {
        let store = match app.store(STORE_FILE) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        store.get(STORE_KEY)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    pub fn save(app: &AppHandle, files: &[RecentFile]) -> Result<(), AppError> {
        let store = app.store(STORE_FILE).map_err(|e| AppError::Internal(e.to_string()))?;
        let value = serde_json::to_value(files).map_err(|e| AppError::Internal(e.to_string()))?;
        store.set(STORE_KEY, value);
        store.save().map_err(|e| AppError::WriteError(e.to_string()))?;
        Ok(())
    }

    pub fn add(app: &AppHandle, file: RecentFile) -> Result<RecentFile, AppError> {
        let mut files = Self::get_all(app);

        let existing_idx = files.iter().position(|f| f.path == file.path);
        if let Some(idx) = existing_idx {
            files[idx].last_opened = file.last_opened;
            files[idx].file_size = file.file_size;
            files[idx].thumbnail = file.thumbnail;
            files.sort_by(|a, b| b.last_opened.cmp(&a.last_opened));
            Self::save(app, &files)?;
            return Ok(files[idx].clone());
        }

        files.insert(0, file);
        files.truncate(MAX_RECENT);

        Self::save(app, &files)?;
        Ok(files[0].clone())
    }

    pub fn remove(app: &AppHandle, id: &str) -> Result<(), AppError> {
        let mut files = Self::get_all(app);
        files.retain(|f| f.id != id);
        Self::save(app, &files)
    }

    pub fn update_path(app: &AppHandle, id: &str, new_path: &str) -> Result<(), AppError> {
        let mut files = Self::get_all(app);

        let file = files
            .iter_mut()
            .find(|f| f.id == id)
            .ok_or_else(|| AppError::FileNotFound(id.to_string()))?;

        file.path = new_path.to_string();

        if let Some(name) = std::path::Path::new(new_path).file_name() {
            file.file_name = name.to_string_lossy().to_string();
        }

        files.sort_by(|a, b| b.last_opened.cmp(&a.last_opened));

        Self::save(app, &files)
    }

    pub fn exists(path: &str) -> bool {
        if path.starts_with("content://") || path.starts_with("file://") || path.starts_with("blob:") {
            return true;
        }
        std::path::Path::new(path).exists()
    }
}