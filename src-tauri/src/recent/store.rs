use std::cmp::Reverse;
use std::sync::{Mutex, OnceLock};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

use super::types::RecentFile;
use crate::error::AppError;

const STORE_FILE: &str = "recent-files.json";
const STORE_KEY: &str = "recent_files";
const MAX_RECENT: usize = 10;
static RECENT_STORE_TRANSACTION: OnceLock<Mutex<()>> = OnceLock::new();

pub struct RecentStore;

impl RecentStore {
    pub fn get_all(app: &AppHandle) -> Vec<RecentFile> {
        with_store_transaction(|| Ok(Self::get_all_unlocked(app))).unwrap_or_default()
    }

    fn get_all_unlocked(app: &AppHandle) -> Vec<RecentFile> {
        let store = match app.store(STORE_FILE) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        store
            .get(STORE_KEY)
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }

    fn save_unlocked(app: &AppHandle, files: &[RecentFile]) -> Result<(), AppError> {
        let store = app
            .store(STORE_FILE)
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let value = serde_json::to_value(files).map_err(|e| AppError::Internal(e.to_string()))?;
        let previous = store.get(STORE_KEY);
        store.set(STORE_KEY, value);
        if let Err(error) = store.save() {
            match previous {
                Some(value) => store.set(STORE_KEY, value),
                None => {
                    store.delete(STORE_KEY);
                }
            }
            return Err(AppError::WriteError(error.to_string()));
        }
        Ok(())
    }

    pub fn add(app: &AppHandle, file: RecentFile) -> Result<RecentFile, AppError> {
        with_store_transaction(|| {
            let (files, updated) = upsert_recent_file(Self::get_all_unlocked(app), file);
            Self::save_unlocked(app, &files)?;
            Ok(updated)
        })
    }

    pub fn remove(app: &AppHandle, id: &str) -> Result<(), AppError> {
        with_store_transaction(|| {
            let mut files = Self::get_all_unlocked(app);
            files.retain(|f| f.id != id);
            Self::save_unlocked(app, &files)
        })
    }
}

fn with_store_transaction<T>(action: impl FnOnce() -> Result<T, AppError>) -> Result<T, AppError> {
    let _guard = RECENT_STORE_TRANSACTION
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| AppError::poisoned_lock("recent file store transaction"))?;
    action()
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

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

    #[test]
    fn recent_store_transactions_serialize_concurrent_callers() {
        const CALLERS: usize = 4;
        let barrier = Arc::new(Barrier::new(CALLERS));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let handles = (0..CALLERS)
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                let active = Arc::clone(&active);
                let max_active = Arc::clone(&max_active);
                thread::spawn(move || {
                    barrier.wait();
                    with_store_transaction(|| {
                        let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                        max_active.fetch_max(current, Ordering::SeqCst);
                        thread::sleep(Duration::from_millis(5));
                        active.fetch_sub(1, Ordering::SeqCst);
                        Ok(())
                    })
                    .expect("recent store transaction");
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().expect("transaction caller");
        }

        assert_eq!(max_active.load(Ordering::SeqCst), 1);
    }
}
