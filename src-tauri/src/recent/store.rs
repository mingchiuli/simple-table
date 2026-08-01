use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

use super::model::{RecentFileRecord, RecentStorageType};
use crate::error::AppError;
use crate::io::atomic_file::{replace_temp_file, write_file_atomically};

const STORE_FILE: &str = "recent-files.json";
const STORE_KEY: &str = "recent_files";
const MAX_RECENT: usize = 10;
const MAX_RECENT_THUMBNAILS: usize = 10;
const MAX_STORED_RECENT_FILES: usize = 1_024;
const MAX_RECENT_STORE_BYTES: usize = 4 * 1024 * 1024;
const MAX_RECENT_ID_BYTES: usize = 256;
const MAX_RECENT_PATH_BYTES: usize = 16 * 1024;
const MAX_RECENT_FILE_NAME_BYTES: usize = 1_024;
const MAX_RECENT_THUMBNAIL_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedRecentFile {
    id: String,
    path: String,
    file_name: String,
    last_opened: i64,
    file_size: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    thumbnail: Option<String>,
    #[serde(default)]
    storage_type: PersistedStorageType,
    #[serde(skip_serializing_if = "Option::is_none")]
    original_path: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum PersistedStorageType {
    MobileSandboxPath,
    #[default]
    DesktopPath,
}

impl From<PersistedRecentFile> for RecentFileRecord {
    fn from(value: PersistedRecentFile) -> Self {
        Self {
            id: value.id,
            path: value.path,
            file_name: value.file_name,
            last_opened: value.last_opened,
            file_size: value.file_size,
            thumbnail: value.thumbnail,
            storage_type: match value.storage_type {
                PersistedStorageType::MobileSandboxPath => RecentStorageType::MobileSandboxPath,
                PersistedStorageType::DesktopPath => RecentStorageType::DesktopPath,
            },
            original_path: value.original_path,
        }
    }
}

impl From<&RecentFileRecord> for PersistedRecentFile {
    fn from(value: &RecentFileRecord) -> Self {
        Self {
            id: value.id.clone(),
            path: value.path.clone(),
            file_name: value.file_name.clone(),
            last_opened: value.last_opened,
            file_size: value.file_size,
            thumbnail: value.thumbnail.clone(),
            storage_type: match value.storage_type {
                RecentStorageType::MobileSandboxPath => PersistedStorageType::MobileSandboxPath,
                RecentStorageType::DesktopPath => PersistedStorageType::DesktopPath,
            },
            original_path: value.original_path.clone(),
        }
    }
}

#[derive(Clone, Default)]
pub struct RecentStore {
    transaction: Arc<Mutex<()>>,
}

pub struct RecentStoreUpdate {
    pub updated: RecentFileRecord,
    pub removed: Vec<RecentFileRecord>,
}

impl RecentStore {
    pub fn get_all(&self, app: &AppHandle) -> Result<Vec<RecentFileRecord>, AppError> {
        self.with_transaction(|| Self::get_all_unlocked(app))
    }

    fn get_all_unlocked(app: &AppHandle) -> Result<Vec<RecentFileRecord>, AppError> {
        load_recent_files(&recent_store_path(app)?)
    }

    fn save_unlocked(app: &AppHandle, files: &[RecentFileRecord]) -> Result<(), AppError> {
        save_recent_files(&recent_store_path(app)?, files)
    }

    pub fn add(
        &self,
        app: &AppHandle,
        file: RecentFileRecord,
    ) -> Result<RecentStoreUpdate, AppError> {
        self.with_transaction(|| {
            let previous = Self::get_all_unlocked(app)?;
            let (files, updated) = upsert_recent_file(previous.clone(), file);
            let removed = removed_recent_files(previous, &files);
            Self::save_unlocked(app, &files)?;
            Ok(RecentStoreUpdate { updated, removed })
        })
    }

    pub fn remove(&self, app: &AppHandle, id: &str) -> Result<Vec<RecentFileRecord>, AppError> {
        self.with_transaction(|| {
            let mut files = Self::get_all_unlocked(app)?;
            let removed = files.iter().filter(|file| file.id == id).cloned().collect();
            files.retain(|f| f.id != id);
            Self::save_unlocked(app, &files)?;
            Ok(removed)
        })
    }

    #[cfg(any(target_os = "android", target_os = "ios", test))]
    #[cfg_attr(test, allow(dead_code))]
    pub fn replace_all(
        &self,
        app: &AppHandle,
        mut files: Vec<RecentFileRecord>,
    ) -> Result<(), AppError> {
        self.with_transaction(|| {
            files.sort_by_key(|file| Reverse(file.last_opened));
            limit_desktop_recents(&mut files, "");
            limit_managed_thumbnails(&mut files);
            Self::save_unlocked(app, &files)
        })
    }

    fn with_transaction<T>(
        &self,
        action: impl FnOnce() -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let _guard = self
            .transaction
            .lock()
            .map_err(|_| AppError::poisoned_lock("recent file store transaction"))?;
        action()
    }

    #[cfg(test)]
    pub(crate) fn is_same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.transaction, &other.transaction)
    }
}

fn recent_store_path(app: &AppHandle) -> Result<PathBuf, AppError> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(STORE_FILE))
        .map_err(|error| {
            AppError::Internal(format!("Failed to resolve recent file store path: {error}"))
        })
}

fn load_recent_files(path: &Path) -> Result<Vec<RecentFileRecord>, AppError> {
    let Some(bytes) = read_recent_store_bytes(path)? else {
        return Ok(Vec::new());
    };
    decode_recent_store_bytes(&bytes)
        .map_err(|error| quarantine_invalid_recent_store(path, error.to_string()))
}

fn read_recent_store_bytes(path: &Path) -> Result<Option<Vec<u8>>, AppError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(AppError::ReadError(format!(
                "Failed to open recent file store: {error}"
            )));
        }
    };
    let metadata = file.metadata().map_err(|error| {
        AppError::ReadError(format!("Failed to inspect recent file store: {error}"))
    })?;
    if metadata.len() > MAX_RECENT_STORE_BYTES as u64 {
        return Err(quarantine_invalid_recent_store(
            path,
            format!(
                "Invalid recent file store: {} bytes exceeds the limit of {MAX_RECENT_STORE_BYTES}",
                metadata.len()
            ),
        ));
    }

    let capacity = usize::try_from(metadata.len())
        .unwrap_or(MAX_RECENT_STORE_BYTES)
        .min(MAX_RECENT_STORE_BYTES);
    let mut bytes = Vec::with_capacity(capacity);
    file.take((MAX_RECENT_STORE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            AppError::ReadError(format!("Failed to read recent file store: {error}"))
        })?;
    if bytes.len() > MAX_RECENT_STORE_BYTES {
        return Err(quarantine_invalid_recent_store(
            path,
            format!(
                "Invalid recent file store: more than {MAX_RECENT_STORE_BYTES} bytes were read"
            ),
        ));
    }
    Ok(Some(bytes))
}

fn decode_recent_store_bytes(bytes: &[u8]) -> Result<Vec<RecentFileRecord>, AppError> {
    let mut store: serde_json::Map<String, serde_json::Value> = serde_json::from_slice(bytes)
        .map_err(|error| AppError::ReadError(format!("Invalid recent file store: {error}")))?;
    decode_recent_files(store.remove(STORE_KEY))
}

fn save_recent_files(path: &Path, files: &[RecentFileRecord]) -> Result<(), AppError> {
    let bytes = encode_recent_store(files)?;
    let parent = path.parent().ok_or_else(|| {
        AppError::WriteError("Recent file store path has no parent directory".to_string())
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        AppError::WriteError(format!(
            "Failed to create recent file store directory: {error}"
        ))
    })?;
    write_file_atomically(path, &bytes)
}

fn encode_recent_store(files: &[RecentFileRecord]) -> Result<Vec<u8>, AppError> {
    validate_recent_files(files).map_err(AppError::WriteError)?;
    let persisted = files
        .iter()
        .map(PersistedRecentFile::from)
        .collect::<Vec<_>>();
    let value =
        serde_json::to_value(persisted).map_err(|error| AppError::Internal(error.to_string()))?;
    let mut store = serde_json::Map::new();
    store.insert(STORE_KEY.to_string(), value);
    let bytes =
        serde_json::to_vec_pretty(&store).map_err(|error| AppError::Internal(error.to_string()))?;
    if bytes.len() > MAX_RECENT_STORE_BYTES {
        return Err(AppError::WriteError(format!(
            "Recent file store requires {} bytes; the maximum is {MAX_RECENT_STORE_BYTES}",
            bytes.len()
        )));
    }
    Ok(bytes)
}

fn quarantine_invalid_recent_store(path: &Path, reason: String) -> AppError {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| STORE_FILE.to_string());
    let quarantine = path.with_file_name(format!("{file_name}.corrupt"));
    match replace_temp_file(path, &quarantine) {
        Ok(()) => AppError::ReadError(format!(
            "{reason}; the invalid store was moved to {}",
            quarantine.display()
        )),
        Err(error) => AppError::ReadError(format!(
            "{reason}; failed to quarantine the invalid store: {error}"
        )),
    }
}

fn decode_recent_files(
    value: Option<serde_json::Value>,
) -> Result<Vec<RecentFileRecord>, AppError> {
    match value {
        Some(value) => {
            let files: Vec<PersistedRecentFile> =
                serde_json::from_value(value).map_err(|error| {
                    AppError::ReadError(format!("Invalid recent file store: {error}"))
                })?;
            let files = files
                .into_iter()
                .map(RecentFileRecord::from)
                .collect::<Vec<_>>();
            validate_recent_files(&files).map_err(AppError::ReadError)?;
            Ok(files)
        }
        None => Ok(Vec::new()),
    }
}

pub(crate) fn validate_recent_files(files: &[RecentFileRecord]) -> Result<(), String> {
    if files.len() > MAX_STORED_RECENT_FILES {
        return Err(format!(
            "Invalid recent file store: {} records exceeds the limit of {MAX_STORED_RECENT_FILES}",
            files.len()
        ));
    }

    let mut total_bytes = 0usize;
    for file in files {
        validate_field("id", &file.id, MAX_RECENT_ID_BYTES)?;
        validate_field("path", &file.path, MAX_RECENT_PATH_BYTES)?;
        validate_field("file name", &file.file_name, MAX_RECENT_FILE_NAME_BYTES)?;
        if let Some(original_path) = &file.original_path {
            validate_field("original path", original_path, MAX_RECENT_PATH_BYTES)?;
        }
        if let Some(thumbnail) = &file.thumbnail {
            validate_field("thumbnail", thumbnail, MAX_RECENT_THUMBNAIL_BYTES)?;
        }
        total_bytes = total_bytes
            .checked_add(recent_file_text_bytes(file))
            .ok_or_else(|| "Invalid recent file store: byte count overflowed".to_string())?;
        if total_bytes > MAX_RECENT_STORE_BYTES {
            return Err(format!(
                "Invalid recent file store: {total_bytes} text bytes exceeds the limit of {MAX_RECENT_STORE_BYTES}"
            ));
        }
    }
    Ok(())
}

fn validate_field(label: &str, value: &str, maximum_bytes: usize) -> Result<(), String> {
    if value.len() > maximum_bytes {
        return Err(format!(
            "Invalid recent file store: {label} requires {} bytes, maximum is {maximum_bytes}",
            value.len()
        ));
    }
    Ok(())
}

fn recent_file_text_bytes(file: &RecentFileRecord) -> usize {
    file.id
        .len()
        .saturating_add(file.path.len())
        .saturating_add(file.file_name.len())
        .saturating_add(file.thumbnail.as_ref().map_or(0, String::len))
        .saturating_add(file.original_path.as_ref().map_or(0, String::len))
        .saturating_add(std::mem::size_of::<RecentFileRecord>())
}

fn upsert_recent_file(
    mut files: Vec<RecentFileRecord>,
    file: RecentFileRecord,
) -> (Vec<RecentFileRecord>, RecentFileRecord) {
    let path = file.path.clone();
    let stable_id = files
        .iter()
        .find(|existing| existing.path == path)
        .map(|existing| existing.id.clone())
        .unwrap_or_else(|| file.id.clone());
    let retained_original_path = files
        .iter()
        .find(|existing| existing.path == path)
        .and_then(|existing| existing.original_path.clone());
    let original_path = file.original_path.clone().or(retained_original_path);
    let merged = RecentFileRecord {
        id: stable_id,
        original_path,
        ..file
    };
    let updated = merged.clone();

    files.retain(|existing| existing.path != path);
    files.push(merged);

    files.sort_by_key(|file| Reverse(file.last_opened));
    limit_desktop_recents(&mut files, &path);
    limit_managed_thumbnails(&mut files);

    if !files.iter().any(|file| file.path == path) {
        files.push(updated.clone());
        files.sort_by_key(|file| Reverse(file.last_opened));
        limit_desktop_recents(&mut files, &path);
        limit_managed_thumbnails(&mut files);
    }

    (files, updated)
}

fn limit_managed_thumbnails(files: &mut [RecentFileRecord]) {
    let mut retained = 0;
    for file in files {
        if file.storage_type != RecentStorageType::MobileSandboxPath {
            continue;
        }
        retained += 1;
        if retained > MAX_RECENT_THUMBNAILS {
            file.thumbnail = None;
        }
    }
}

fn limit_desktop_recents(files: &mut Vec<RecentFileRecord>, path: &str) {
    let mut managed = Vec::new();
    let mut desktop = Vec::new();
    for file in files.drain(..) {
        match file.storage_type {
            RecentStorageType::MobileSandboxPath => managed.push(file),
            RecentStorageType::DesktopPath => desktop.push(file),
        }
    }
    truncate_preserving_path(&mut desktop, path);
    managed.extend(desktop);
    managed.sort_by_key(|file| Reverse(file.last_opened));
    *files = managed;
}

fn removed_recent_files(
    previous: Vec<RecentFileRecord>,
    current: &[RecentFileRecord],
) -> Vec<RecentFileRecord> {
    let retained_paths: HashSet<&str> = current.iter().map(|file| file.path.as_str()).collect();
    previous
        .into_iter()
        .filter(|file| !retained_paths.contains(file.path.as_str()))
        .collect()
}

fn truncate_preserving_path(files: &mut Vec<RecentFileRecord>, path: &str) {
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    fn recent(id: &str, path: &str, file_name: &str, last_opened: i64) -> RecentFileRecord {
        RecentFileRecord {
            id: id.to_string(),
            path: path.to_string(),
            file_name: file_name.to_string(),
            last_opened,
            file_size: 1,
            thumbnail: None,
            storage_type: RecentStorageType::DesktopPath,
            original_path: None,
        }
    }

    fn encoded_recent_files(files: &[RecentFileRecord]) -> serde_json::Value {
        serde_json::to_value(
            files
                .iter()
                .map(PersistedRecentFile::from)
                .collect::<Vec<_>>(),
        )
        .expect("persisted recent files")
    }

    fn temp_store_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "simple-table-recent-{label}-{}.json",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn missing_recent_store_value_is_an_empty_list() {
        assert!(decode_recent_files(None).unwrap().is_empty());
    }

    #[test]
    fn malformed_recent_store_value_is_not_treated_as_an_empty_list() {
        let error = decode_recent_files(Some(serde_json::json!({ "invalid": true })))
            .expect_err("malformed recent data must fail");

        assert!(matches!(error, AppError::ReadError(_)));
    }

    #[test]
    fn persisted_recent_schema_remains_wire_compatible() {
        let mut file = recent("id", "/tmp/book.xlsx", "book.xlsx", 7);
        file.original_path = Some("/import/book.xlsx".to_string());
        let encoded = encoded_recent_files(&[file]);

        assert_eq!(encoded[0]["id"], "id");
        assert_eq!(encoded[0]["fileName"], "book.xlsx");
        assert_eq!(encoded[0]["lastOpened"], 7);
        assert_eq!(encoded[0]["storageType"], "desktopPath");
        assert_eq!(encoded[0]["originalPath"], "/import/book.xlsx");
        assert_eq!(decode_recent_files(Some(encoded)).unwrap().len(), 1);
    }

    #[test]
    fn direct_store_codec_reads_the_plugin_store_schema() {
        let file = recent("id", "/tmp/book.xlsx", "book.xlsx", 7);
        let mut store = serde_json::Map::new();
        store.insert(STORE_KEY.to_string(), encoded_recent_files(&[file]));
        let bytes = serde_json::to_vec(&store).expect("store bytes");

        let decoded = decode_recent_store_bytes(&bytes).expect("compatible store");

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].id, "id");
        assert_eq!(decoded[0].path, "/tmp/book.xlsx");
    }

    #[test]
    fn oversized_store_is_quarantined_before_json_parsing() {
        let path = temp_store_path("oversized");
        let quarantine = path.with_file_name(format!(
            "{}.corrupt",
            path.file_name().unwrap().to_string_lossy()
        ));
        fs::write(&path, vec![b'x'; MAX_RECENT_STORE_BYTES + 1]).expect("oversized store");

        let error = load_recent_files(&path).expect_err("oversized store must fail");

        assert!(error.to_string().contains("exceeds the limit"));
        assert!(!path.exists());
        assert_eq!(
            fs::metadata(&quarantine).expect("quarantined store").len(),
            (MAX_RECENT_STORE_BYTES + 1) as u64
        );
        let _ = fs::remove_file(quarantine);
    }

    #[test]
    fn malformed_store_replaces_the_single_quarantine_backup() {
        let path = temp_store_path("malformed");
        let quarantine = path.with_file_name(format!(
            "{}.corrupt",
            path.file_name().unwrap().to_string_lossy()
        ));
        fs::write(&path, b"{").expect("malformed store");
        fs::write(&quarantine, b"older corruption").expect("older quarantine");

        let error = load_recent_files(&path).expect_err("malformed store must fail");

        assert!(error.to_string().contains("Invalid recent file store"));
        assert!(!path.exists());
        assert_eq!(fs::read(&quarantine).expect("quarantine bytes"), b"{");
        let _ = fs::remove_file(quarantine);
    }

    #[test]
    fn encoded_size_limit_preserves_the_previous_store() {
        let path = temp_store_path("encoded-limit");
        let previous = br#"{"recent_files":[]}"#;
        fs::write(&path, previous).expect("previous store");
        let files = (0..3)
            .map(|index| {
                let mut file = recent(
                    &format!("id-{index}"),
                    &format!("/tmp/{index}.xlsx"),
                    &format!("{index}.xlsx"),
                    index,
                );
                file.thumbnail = Some("\0".repeat(MAX_RECENT_THUMBNAIL_BYTES));
                file
            })
            .collect::<Vec<_>>();

        let error = save_recent_files(&path, &files).expect_err("encoded store must be bounded");

        assert!(error.to_string().contains("maximum"));
        assert_eq!(fs::read(&path).expect("previous store"), previous);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn recent_store_rejects_excessive_records() {
        let files = (0..=MAX_STORED_RECENT_FILES)
            .map(|index| {
                recent(
                    &format!("id-{index}"),
                    &format!("/tmp/{index}.xlsx"),
                    &format!("{index}.xlsx"),
                    index as i64,
                )
            })
            .collect::<Vec<_>>();

        let error = decode_recent_files(Some(encoded_recent_files(&files)))
            .expect_err("oversized recent store");

        assert!(matches!(error, AppError::ReadError(_)));
    }

    #[test]
    fn recent_store_rejects_oversized_thumbnail_metadata() {
        let mut file = recent("id", "/tmp/book.xlsx", "book.xlsx", 1);
        file.thumbnail = Some("x".repeat(MAX_RECENT_THUMBNAIL_BYTES + 1));

        let error = decode_recent_files(Some(encoded_recent_files(&[file])))
            .expect_err("oversized thumbnail");

        assert!(matches!(error, AppError::ReadError(_)));
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
    fn upsert_preserves_original_path_when_a_save_omits_it() {
        let mut existing = recent("stable-id", "/tmp/book.xlsx", "book.xlsx", 1);
        existing.original_path = Some("/import/book.xlsx".to_string());
        let updated_input = recent("new-id", "/tmp/book.xlsx", "book.xlsx", 2);

        let (files, updated) = upsert_recent_file(vec![existing], updated_input);

        assert_eq!(files[0].original_path.as_deref(), Some("/import/book.xlsx"));
        assert_eq!(updated.original_path.as_deref(), Some("/import/book.xlsx"));
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
    fn removed_recent_files_reports_evicted_paths_but_not_updated_duplicates() {
        let evicted = recent("evicted", "/tmp/evicted.xlsx", "evicted.xlsx", 0);
        let retained = recent("retained", "/tmp/retained.xlsx", "retained.xlsx", 1);
        let updated = recent("updated", "/tmp/retained.xlsx", "renamed.xlsx", 2);

        let removed = removed_recent_files(
            vec![evicted.clone(), retained],
            std::slice::from_ref(&updated),
        );

        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].path, evicted.path);
    }

    #[test]
    fn mobile_managed_documents_are_not_evicted_by_the_desktop_recent_limit() {
        let mut files = (0..=MAX_RECENT)
            .map(|index| {
                let mut file = recent(
                    &format!("mobile-{index}"),
                    &format!("/mobile/{index}.xlsx"),
                    &format!("{index}.xlsx"),
                    index as i64,
                );
                file.storage_type = RecentStorageType::MobileSandboxPath;
                file
            })
            .collect::<Vec<_>>();
        let newest = files.pop().expect("newest managed document");

        let (updated_files, _) = upsert_recent_file(files, newest);

        assert_eq!(updated_files.len(), MAX_RECENT + 1);
        assert!(
            updated_files
                .iter()
                .all(|file| file.storage_type == RecentStorageType::MobileSandboxPath)
        );
    }

    #[test]
    fn mobile_thumbnail_metadata_is_bounded_without_evicting_documents() {
        let mut files = (0..=MAX_RECENT_THUMBNAILS)
            .map(|index| {
                let mut file = recent(
                    &format!("mobile-{index}"),
                    &format!("/mobile/{index}.xlsx"),
                    &format!("{index}.xlsx"),
                    index as i64,
                );
                file.storage_type = RecentStorageType::MobileSandboxPath;
                file.thumbnail = Some(format!("thumbnail-{index}"));
                file
            })
            .collect::<Vec<_>>();
        files.sort_by_key(|file| Reverse(file.last_opened));

        limit_managed_thumbnails(&mut files);

        assert_eq!(files.len(), MAX_RECENT_THUMBNAILS + 1);
        assert_eq!(
            files.iter().filter(|file| file.thumbnail.is_some()).count(),
            MAX_RECENT_THUMBNAILS
        );
    }

    #[test]
    fn recent_store_transactions_serialize_concurrent_callers() {
        const CALLERS: usize = 4;
        let barrier = Arc::new(Barrier::new(CALLERS));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let store = RecentStore::default();
        let handles = (0..CALLERS)
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                let active = Arc::clone(&active);
                let max_active = Arc::clone(&max_active);
                let store = store.clone();
                thread::spawn(move || {
                    barrier.wait();
                    store
                        .with_transaction(|| {
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
