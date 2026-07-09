use std::cmp::Reverse;
use std::path::Path;

use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

use crate::error::AppError;
use crate::io::file_format::file_name_from_path_like;

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
        store
            .get(STORE_KEY)
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }

    pub fn save(app: &AppHandle, files: &[RecentFile]) -> Result<(), AppError> {
        let store = app
            .store(STORE_FILE)
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let value = serde_json::to_value(files).map_err(|e| AppError::Internal(e.to_string()))?;
        store.set(STORE_KEY, value);
        store
            .save()
            .map_err(|e| AppError::WriteError(e.to_string()))?;
        Ok(())
    }

    pub fn add(app: &AppHandle, file: RecentFile) -> Result<RecentFile, AppError> {
        let (files, updated) = upsert_recent_file(Self::get_all(app), file);
        Self::save(app, &files)?;
        Ok(updated)
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
        file.file_name = file_name_from_path_like(new_path, &file.file_name);

        files.sort_by_key(|file| Reverse(file.last_opened));

        Self::save(app, &files)
    }

    pub fn exists(path: &str) -> bool {
        let lower_path = path.to_ascii_lowercase();
        if lower_path.starts_with("content://")
            || lower_path.starts_with("file://")
            || lower_path.starts_with("blob:")
        {
            return true;
        }
        Path::new(path).exists()
    }
}

fn upsert_recent_file(
    mut files: Vec<RecentFile>,
    file: RecentFile,
) -> (Vec<RecentFile>, RecentFile) {
    let path = file.path.clone();
    let stable_id = files
        .iter()
        .find(|existing| existing.path == path)
        .map(|existing| existing.id.clone())
        .unwrap_or_else(|| file.id.clone());
    let merged = RecentFile {
        id: stable_id,
        ..file
    };
    let updated = merged.clone();

    files.retain(|existing| existing.path != path);
    files.push(merged);

    files.sort_by_key(|file| Reverse(file.last_opened));
    truncate_preserving_path(&mut files, &path);

    if !files.iter().any(|file| file.path == path) {
        files.push(updated.clone());
        files.sort_by_key(|file| Reverse(file.last_opened));
        truncate_preserving_path(&mut files, &path);
    }

    (files, updated)
}

fn truncate_preserving_path(files: &mut Vec<RecentFile>, path: &str) {
    if files.len() <= MAX_RECENT {
        return;
    }

    if files.iter().take(MAX_RECENT).any(|file| file.path == path) {
        files.truncate(MAX_RECENT);
        return;
    }

    let Some(upserted_index) = files.iter().position(|file| file.path == path) else {
        files.truncate(MAX_RECENT);
        return;
    };
    let upserted = files.remove(upserted_index);
    files.truncate(MAX_RECENT - 1);
    files.push(upserted);
    files.sort_by_key(|file| Reverse(file.last_opened));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recent::types::StorageType;

    fn recent(id: &str, path: &str, file_name: &str, last_opened: i64) -> RecentFile {
        RecentFile {
            id: id.to_string(),
            path: path.to_string(),
            file_name: file_name.to_string(),
            last_opened,
            file_size: 1,
            thumbnail: None,
            storage_type: StorageType::DesktopPath,
            original_path: None,
        }
    }

    #[test]
    fn upsert_updates_existing_recent_file_metadata_without_changing_id() {
        let existing = recent("stable-id", "/tmp/book.xlsx", "old.xlsx", 1);
        let mut updated_input = recent("new-id", "/tmp/book.xlsx", "renamed.xlsx", 2);
        updated_input.file_size = 99;
        updated_input.thumbnail = Some("thumb".to_string());
        updated_input.original_path = Some("/original/renamed.xlsx".to_string());

        let (files, updated) = upsert_recent_file(vec![existing], updated_input);

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].id, "stable-id");
        assert_eq!(files[0].file_name, "renamed.xlsx");
        assert_eq!(files[0].file_size, 99);
        assert_eq!(files[0].thumbnail.as_deref(), Some("thumb"));
        assert_eq!(
            files[0].original_path.as_deref(),
            Some("/original/renamed.xlsx")
        );
        assert_eq!(updated.id, "stable-id");
        assert_eq!(updated.file_name, "renamed.xlsx");
    }

    #[test]
    fn virtual_uri_exists_checks_are_case_insensitive() {
        assert!(RecentStore::exists("content://provider/document"));
        assert!(RecentStore::exists("CONTENT://provider/document"));
        assert!(RecentStore::exists("FILE://server/share/book.xlsx"));
        assert!(RecentStore::exists("BLOB:temporary-id"));
    }

    #[test]
    fn upsert_collapses_duplicate_paths_from_older_stores() {
        let older = recent("oldest-id", "/tmp/book.xlsx", "oldest.xlsx", 1);
        let newer_duplicate = recent("duplicate-id", "/tmp/book.xlsx", "duplicate.xlsx", 2);
        let other = recent("other-id", "/tmp/other.xlsx", "other.xlsx", 3);
        let updated_input = recent("new-id", "/tmp/book.xlsx", "current.xlsx", 4);

        let (files, updated) =
            upsert_recent_file(vec![older, newer_duplicate, other], updated_input);

        let matching = files
            .iter()
            .filter(|file| file.path == "/tmp/book.xlsx")
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].id, "oldest-id");
        assert_eq!(matching[0].file_name, "current.xlsx");
        assert_eq!(updated.id, "oldest-id");
        assert_eq!(updated.file_name, "current.xlsx");
    }

    #[test]
    fn upsert_sorts_recent_files_and_limits_history() {
        let mut files = (0..MAX_RECENT)
            .map(|index| {
                recent(
                    &format!("id-{index}"),
                    &format!("/tmp/{index}.xlsx"),
                    &format!("{index}.xlsx"),
                    index as i64,
                )
            })
            .collect::<Vec<_>>();

        let newest = recent("new", "/tmp/new.xlsx", "new.xlsx", 100);
        let (updated_files, updated) = upsert_recent_file(std::mem::take(&mut files), newest);

        assert_eq!(updated_files.len(), MAX_RECENT);
        assert_eq!(updated_files[0].path, "/tmp/new.xlsx");
        assert_eq!(updated.path, "/tmp/new.xlsx");
        assert!(!updated_files.iter().any(|file| file.path == "/tmp/0.xlsx"));
    }

    #[test]
    fn upsert_retains_current_file_when_clock_moves_backwards() {
        let files = (0..MAX_RECENT)
            .map(|index| {
                recent(
                    &format!("id-{index}"),
                    &format!("/tmp/{index}.xlsx"),
                    &format!("{index}.xlsx"),
                    100 + index as i64,
                )
            })
            .collect::<Vec<_>>();

        let current = recent("new", "/tmp/new.xlsx", "new.xlsx", 1);
        let (updated_files, updated) = upsert_recent_file(files, current);

        assert_eq!(updated.path, "/tmp/new.xlsx");
        assert!(
            updated_files
                .iter()
                .any(|file| file.path == "/tmp/new.xlsx")
        );
        assert_eq!(updated_files.len(), MAX_RECENT);
    }
}
