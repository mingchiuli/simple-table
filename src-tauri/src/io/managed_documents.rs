use crate::error::AppError;
use crate::io::atomic_file::write_file_atomically;
use crate::io::marker_store::{
    bounded_directory_entries, read_marker_bytes, validate_marker_field,
};
use crate::io::transient_files::{clear_persistent_marker, transient_file_registry};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_MANAGED_DOCUMENTS: usize = 64;
const MAX_MANAGED_DOCUMENT_BYTES: u64 = 1024 * 1024 * 1024;
const MARKER_PREFIX: &str = ".simple-table-managed-";
const MARKER_SUFFIX: &str = ".json";

static MANAGED_DOCUMENT_TRANSACTION: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManagedDocumentRecord {
    pub(crate) id: String,
    pub(crate) path: PathBuf,
    pub(crate) file_name: String,
    pub(crate) adopted_at_millis: i64,
    pub(crate) file_size: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistentManagedDocument {
    id: String,
    target_file_name: String,
    file_name: String,
    adopted_at_millis: i64,
}

pub(crate) fn adopt_transient_document(target: &Path, file_name: &str) -> Result<bool, AppError> {
    if !transient_file_registry().contains(target)? {
        return Ok(false);
    }
    persist_managed_document(target, file_name, None, None, true)?;
    transient_file_registry().adopt_if_registered(target)?;
    Ok(true)
}

pub(crate) fn adopt_completed_save(target: &Path, file_name: &str) -> Result<(), AppError> {
    persist_managed_document(target, file_name, None, None, true)?;
    transient_file_registry().adopt_if_registered(target)?;
    Ok(())
}

pub(crate) fn validate_managed_save(target: &Path, future_size: u64) -> Result<(), AppError> {
    let _guard = transaction_lock()?;
    let directory = target.parent().ok_or_else(|| {
        AppError::DocumentStateInvalid("managed document path has no parent".to_string())
    })?;
    let records = scan_managed_documents(directory, true)?;
    let existing = records.iter().find(|record| record.path == target);
    if existing.is_none() && records.len() >= MAX_MANAGED_DOCUMENTS {
        return Err(AppError::ResourceLimitExceeded(format!(
            "at most {MAX_MANAGED_DOCUMENTS} managed mobile documents may be retained"
        )));
    }
    let current_total = records.iter().map(|record| record.file_size).sum::<u64>();
    let next_total = current_total
        .saturating_sub(existing.map(|record| record.file_size).unwrap_or(0))
        .saturating_add(future_size);
    if next_total > MAX_MANAGED_DOCUMENT_BYTES && next_total > current_total {
        return Err(AppError::ResourceLimitExceeded(format!(
            "managed mobile documents would require {next_total} bytes, maximum is {MAX_MANAGED_DOCUMENT_BYTES} bytes"
        )));
    }
    Ok(())
}

pub(crate) fn recover_completed_save(target: &Path) -> Result<(), AppError> {
    let file_name = direct_file_name(target)?;
    persist_managed_document(target, &file_name, None, None, true)?;
    clear_persistent_marker(target);
    Ok(())
}

pub(crate) fn migrate_existing_document(
    target: &Path,
    file_name: &str,
    id: &str,
    adopted_at_millis: i64,
) -> Result<(), AppError> {
    if marker_path(target)?.exists() || !target.exists() {
        return Ok(());
    }
    persist_managed_document(target, file_name, Some(id), Some(adopted_at_millis), false)
}

pub(crate) fn managed_documents(directory: &Path) -> Result<Vec<ManagedDocumentRecord>, AppError> {
    let _guard = transaction_lock()?;
    scan_managed_documents(directory, true)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn reconcile_managed_documents(directory: &Path) -> Result<(), AppError> {
    managed_documents(directory).map(|_| ())
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn clear_managed_document(target: &Path) -> Result<(), AppError> {
    let _guard = transaction_lock()?;
    match fs::remove_file(marker_path(target)?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::WriteError(format!(
            "Failed to remove managed document marker: {error}"
        ))),
    }
}

fn persist_managed_document(
    target: &Path,
    file_name: &str,
    requested_id: Option<&str>,
    adopted_at_millis: Option<i64>,
    enforce_capacity: bool,
) -> Result<(), AppError> {
    let _guard = transaction_lock()?;
    let directory = target.parent().ok_or_else(|| {
        AppError::DocumentStateInvalid("managed document path has no parent".to_string())
    })?;
    let target_file_name = direct_file_name(target)?;
    validate_marker_field("target file name", &target_file_name)?;
    validate_marker_field("file name", file_name)?;
    if let Some(id) = requested_id {
        validate_marker_field("id", id)?;
    }
    let file_size = fs::metadata(target)
        .map_err(|error| {
            AppError::ReadError(format!("Failed to inspect managed document: {error}"))
        })?
        .len();
    let existing = read_marker(target).ok();
    let records = scan_managed_documents(directory, true)?;

    if enforce_capacity && existing.is_none() {
        let total_bytes = records
            .iter()
            .map(|record| record.file_size)
            .sum::<u64>()
            .saturating_add(file_size);
        if records.len() >= MAX_MANAGED_DOCUMENTS {
            return Err(AppError::ResourceLimitExceeded(format!(
                "at most {MAX_MANAGED_DOCUMENTS} managed mobile documents may be retained"
            )));
        }
        if total_bytes > MAX_MANAGED_DOCUMENT_BYTES {
            return Err(AppError::ResourceLimitExceeded(format!(
                "managed mobile documents require {total_bytes} bytes, maximum is {MAX_MANAGED_DOCUMENT_BYTES} bytes"
            )));
        }
    }

    let marker = PersistentManagedDocument {
        id: existing
            .as_ref()
            .map(|marker| marker.id.clone())
            .or_else(|| requested_id.map(str::to_string))
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        target_file_name,
        file_name: file_name.to_string(),
        adopted_at_millis: adopted_at_millis
            .or_else(|| existing.as_ref().map(|marker| marker.adopted_at_millis))
            .unwrap_or_else(now_millis),
    };
    let bytes = serde_json::to_vec(&marker).map_err(|error| {
        AppError::Internal(format!("Failed to encode managed document marker: {error}"))
    })?;
    write_file_atomically(&marker_path(target)?, &bytes)
}

fn scan_managed_documents(
    directory: &Path,
    remove_invalid: bool,
) -> Result<Vec<ManagedDocumentRecord>, AppError> {
    let entries = bounded_directory_entries(directory, "managed document")?;
    let mut records = Vec::new();
    for entry in entries {
        let marker_path = entry.path();
        let Some(name) = marker_path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with(MARKER_PREFIX) || !name.ends_with(MARKER_SUFFIX) {
            continue;
        }
        let marker = read_marker_bytes(&marker_path, "managed document marker")
            .ok()
            .and_then(|bytes| serde_json::from_slice::<PersistentManagedDocument>(&bytes).ok())
            .filter(|marker| validate_managed_marker(marker).is_ok());
        let Some(marker) = marker else {
            if remove_invalid {
                let _ = fs::remove_file(marker_path);
            }
            continue;
        };
        let target = directory.join(&marker.target_file_name);
        if direct_file_name(&target).ok().as_deref() != Some(marker.target_file_name.as_str()) {
            if remove_invalid {
                let _ = fs::remove_file(marker_path);
            }
            continue;
        }
        let Ok(metadata) = fs::metadata(&target) else {
            if remove_invalid {
                let _ = fs::remove_file(marker_path);
            }
            continue;
        };
        if !metadata.is_file() {
            if remove_invalid {
                let _ = fs::remove_file(marker_path);
            }
            continue;
        }
        records.push(ManagedDocumentRecord {
            id: marker.id,
            path: target,
            file_name: marker.file_name,
            adopted_at_millis: marker.adopted_at_millis,
            file_size: metadata.len(),
        });
    }
    records.sort_by_key(|record| std::cmp::Reverse(record.adopted_at_millis));
    Ok(records)
}

fn read_marker(target: &Path) -> Result<PersistentManagedDocument, AppError> {
    let bytes = read_marker_bytes(&marker_path(target)?, "managed document marker")?;
    let marker: PersistentManagedDocument = serde_json::from_slice(&bytes).map_err(|error| {
        AppError::ReadError(format!("Invalid managed document marker: {error}"))
    })?;
    validate_managed_marker(&marker)?;
    Ok(marker)
}

fn validate_managed_marker(marker: &PersistentManagedDocument) -> Result<(), AppError> {
    validate_marker_field("id", &marker.id)?;
    validate_marker_field("target file name", &marker.target_file_name)?;
    validate_marker_field("file name", &marker.file_name)
}

fn marker_path(target: &Path) -> Result<PathBuf, AppError> {
    let parent = target.parent().ok_or_else(|| {
        AppError::DocumentStateInvalid("managed document path has no parent".to_string())
    })?;
    let file_name = direct_file_name(target)?;
    let digest = Sha256::digest(file_name.as_bytes());
    let mut digest_hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(digest_hex, "{byte:02x}");
    }
    Ok(parent.join(format!("{MARKER_PREFIX}{digest_hex}{MARKER_SUFFIX}")))
}

fn direct_file_name(path: &Path) -> Result<String, AppError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            AppError::DocumentStateInvalid(
                "managed document path has no valid file name".to_string(),
            )
        })
}

fn transaction_lock() -> Result<std::sync::MutexGuard<'static, ()>, AppError> {
    MANAGED_DOCUMENT_TRANSACTION
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| AppError::poisoned_lock("managed document catalog"))
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "simple-table-managed-{name}-{}",
                uuid::Uuid::new_v4()
            ));
            fs::create_dir_all(&path).expect("test directory");
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn managed_marker_recovers_document_metadata_without_recent_store() {
        let dir = TestDir::new("recover");
        let target = dir.0.join("document.xlsx");
        fs::write(&target, b"workbook").expect("managed file");

        persist_managed_document(&target, "Budget.xlsx", None, Some(42), true)
            .expect("managed marker");
        let records = managed_documents(&dir.0).expect("managed records");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].path, target);
        assert_eq!(records[0].file_name, "Budget.xlsx");
        assert_eq!(records[0].adopted_at_millis, 42);
        assert_eq!(records[0].file_size, 8);
    }

    #[test]
    fn reconciliation_removes_marker_for_missing_document() {
        let dir = TestDir::new("missing");
        let target = dir.0.join("missing.xlsx");
        fs::write(&target, b"workbook").expect("managed file");
        persist_managed_document(&target, "Missing.xlsx", None, None, true)
            .expect("managed marker");
        fs::remove_file(&target).expect("remove target");

        reconcile_managed_documents(&dir.0).expect("reconcile catalog");

        assert!(!marker_path(&target).expect("marker path").exists());
    }

    #[test]
    fn migration_preserves_existing_recent_identity() {
        let dir = TestDir::new("migration");
        let target = dir.0.join("legacy.xlsx");
        fs::write(&target, b"workbook").expect("managed file");

        migrate_existing_document(&target, "Legacy.xlsx", "stable-id", 7)
            .expect("migrate document");
        let records = managed_documents(&dir.0).expect("managed records");

        assert_eq!(records[0].id, "stable-id");
        assert_eq!(records[0].adopted_at_millis, 7);
    }

    #[test]
    fn adopting_transient_document_persists_catalog_before_clearing_transient_marker() {
        let dir = TestDir::new("adoption");
        let target = dir.0.join("imported.xlsx");
        fs::write(&target, b"workbook").expect("transient file");
        transient_file_registry()
            .register(
                target.clone(),
                crate::io::transient_files::TransientFilePurpose::OpenSelection,
            )
            .expect("transient registration");
        crate::io::transient_files::write_persistent_marker(
            &target,
            crate::io::transient_files::TransientFilePurpose::OpenSelection,
        )
        .expect("transient marker");

        assert!(adopt_transient_document(&target, "Imported.xlsx").expect("managed adoption"));

        assert!(!transient_file_registry().contains(&target).unwrap());
        assert_eq!(managed_documents(&dir.0).unwrap().len(), 1);
    }

    #[test]
    fn new_managed_documents_are_rejected_at_the_catalog_count_limit() {
        let dir = TestDir::new("capacity");
        for index in 0..MAX_MANAGED_DOCUMENTS {
            let target = dir.0.join(format!("document-{index}.xlsx"));
            fs::write(&target, b"workbook").expect("managed file");
            persist_managed_document(&target, "Document.xlsx", None, None, true)
                .expect("managed marker");
        }
        let overflow = dir.0.join("overflow.xlsx");
        fs::write(&overflow, b"workbook").expect("overflow file");

        assert!(matches!(
            persist_managed_document(&overflow, "Overflow.xlsx", None, None, true),
            Err(AppError::ResourceLimitExceeded(_))
        ));
        assert_eq!(
            managed_documents(&dir.0).unwrap().len(),
            MAX_MANAGED_DOCUMENTS
        );
    }

    #[test]
    fn save_capacity_allows_existing_replacement_but_rejects_oversized_new_file() {
        let dir = TestDir::new("save-capacity");
        let target = dir.0.join("existing.xlsx");
        fs::write(&target, b"workbook").expect("managed file");
        migrate_existing_document(&target, "Existing.xlsx", "existing", 1).expect("managed marker");

        assert!(validate_managed_save(&target, 1).is_ok());
        assert!(matches!(
            validate_managed_save(&dir.0.join("new.xlsx"), MAX_MANAGED_DOCUMENT_BYTES + 1),
            Err(AppError::ResourceLimitExceeded(_))
        ));
    }
}
