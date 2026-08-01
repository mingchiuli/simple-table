use crate::error::AppError;
use crate::io::atomic_file::{is_owned_temp_file_name, write_file_atomically};
use crate::io::marker_store::{
    bounded_directory_entries, read_marker_bytes, read_optional_marker_bytes, validate_marker_field,
};
use crate::io::transient_files::{
    TransientFilePurpose, TransientFileRegistry, TransientFileReservation, clear_persistent_marker,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_MANAGED_DOCUMENTS: usize = 64;
const MAX_MANAGED_DOCUMENT_BYTES: u64 = 1024 * 1024 * 1024;
const MARKER_PREFIX: &str = ".simple-table-managed-";
const MARKER_SUFFIX: &str = ".json";
const SAVE_TRANSACTION_PREFIX: &str = ".simple-table-managed-save-";
const SAVE_TRANSACTION_SUFFIX: &str = ".json";

#[derive(Clone, Default)]
pub struct ManagedDocumentCatalog {
    transaction: Arc<Mutex<()>>,
}

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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistentManagedSaveTransaction {
    target_file_name: String,
    file_name: String,
    expected_size: u64,
    content_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    temp_file_name: Option<String>,
}

pub(crate) struct ManagedDocumentAdoption {
    catalog: ManagedDocumentCatalog,
    target: PathBuf,
    transient_reservation: Option<TransientFileReservation>,
    previous_managed_marker: Option<Vec<u8>>,
    committed: bool,
}

pub(crate) struct ManagedSaveTransaction {
    catalog: ManagedDocumentCatalog,
    target: PathBuf,
    file_name: String,
    journal: PathBuf,
    transient_reservation: Option<TransientFileReservation>,
    content_committed: bool,
}

impl ManagedSaveTransaction {
    pub(crate) fn attach_transient_reservation(&mut self, reservation: TransientFileReservation) {
        self.transient_reservation = Some(reservation);
    }

    pub(crate) fn finish_after_content_commit(mut self) -> Result<(), AppError> {
        self.content_committed = true;
        let metadata_result = persist_managed_document(
            &self.catalog,
            &self.target,
            &self.file_name,
            None,
            None,
            false,
        );
        let adoption_result = match self.transient_reservation.take() {
            Some(reservation) => {
                reservation.commit();
                Ok(())
            }
            None => Ok(()),
        };
        clear_persistent_marker(&self.target);
        let result = metadata_result.and(adoption_result);
        if result.is_ok() {
            remove_file_if_present(&self.journal, "managed save transaction")?;
        }
        result
    }
}

impl Drop for ManagedSaveTransaction {
    fn drop(&mut self) {
        if !self.content_committed {
            let _ = remove_file_if_present(&self.journal, "managed save transaction");
        }
    }
}

impl ManagedDocumentAdoption {
    pub(crate) fn commit(mut self) {
        if let Some(reservation) = self.transient_reservation.take() {
            reservation.commit();
        }
        self.committed = true;
    }

    fn rollback(&mut self) -> Result<(), AppError> {
        let Some(reservation) = self.transient_reservation.take() else {
            return Ok(());
        };
        let marker_result = restore_managed_marker(
            &self.catalog,
            &self.target,
            self.previous_managed_marker.as_deref(),
        );
        drop(reservation);
        marker_result
    }
}

impl Drop for ManagedDocumentAdoption {
    fn drop(&mut self) {
        if !self.committed {
            let _ = self.rollback();
        }
    }
}

pub(crate) fn begin_transient_document_adoption(
    catalog: &ManagedDocumentCatalog,
    transient_files: Arc<TransientFileRegistry>,
    target: &Path,
    file_name: &str,
) -> Result<ManagedDocumentAdoption, AppError> {
    let transient_reservation =
        transient_files.reserve_if_registered(target, TransientFilePurpose::OpenSelection)?;
    if transient_reservation.is_none() && !target.is_file() {
        return Err(AppError::FileNotFound(target.to_string_lossy().to_string()));
    }
    let previous_managed_marker = if transient_reservation.is_some() {
        marker_path(target)
            .and_then(|path| read_optional_marker_bytes(&path, "managed document marker"))?
    } else {
        None
    };
    if transient_reservation.is_some()
        && let Err(error) = persist_managed_document(catalog, target, file_name, None, None, true)
    {
        return Err(error);
    }
    Ok(ManagedDocumentAdoption {
        catalog: catalog.clone(),
        target: target.to_path_buf(),
        transient_reservation,
        previous_managed_marker,
        committed: false,
    })
}

fn restore_managed_marker(
    catalog: &ManagedDocumentCatalog,
    target: &Path,
    previous: Option<&[u8]>,
) -> Result<(), AppError> {
    let _guard = transaction_lock(catalog)?;
    let path = marker_path(target)?;
    match previous {
        Some(bytes) => write_file_atomically(&path, bytes),
        None => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::WriteError(format!(
                "Failed to roll back managed document marker: {error}"
            ))),
        },
    }
}

pub(crate) fn begin_managed_save_transaction(
    catalog: &ManagedDocumentCatalog,
    target: &Path,
    temp: &Path,
    file_name: &str,
    content: &[u8],
) -> Result<ManagedSaveTransaction, AppError> {
    validate_managed_save(catalog, target, content.len() as u64)?;
    let target_file_name = direct_file_name(target)?;
    let temp_file_name = direct_file_name(temp)?;
    validate_marker_field("target file name", &target_file_name)?;
    validate_transaction_temp_path(target, temp, &temp_file_name)?;
    validate_marker_field("file name", file_name)?;
    let transaction = PersistentManagedSaveTransaction {
        target_file_name,
        file_name: file_name.to_string(),
        expected_size: content.len() as u64,
        content_sha256: sha256_hex(content),
        temp_file_name: Some(temp_file_name),
    };
    let journal = save_transaction_path(target)?;
    let bytes = serde_json::to_vec(&transaction).map_err(|error| {
        AppError::Internal(format!(
            "Failed to encode managed save transaction: {error}"
        ))
    })?;
    write_file_atomically(&journal, &bytes)?;
    Ok(ManagedSaveTransaction {
        catalog: catalog.clone(),
        target: target.to_path_buf(),
        file_name: file_name.to_string(),
        journal,
        transient_reservation: None,
        content_committed: false,
    })
}

pub(crate) fn recover_managed_save_transactions(
    catalog: &ManagedDocumentCatalog,
    directory: &Path,
) -> Result<(), AppError> {
    for entry in bounded_directory_entries(directory, "managed save transaction")? {
        let journal = entry.path();
        let Some(name) = journal.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with(SAVE_TRANSACTION_PREFIX) || !name.ends_with(SAVE_TRANSACTION_SUFFIX) {
            continue;
        }
        let transaction = match read_save_transaction(&journal) {
            Ok(transaction) => transaction,
            Err(_) => {
                let _ = fs::remove_file(&journal);
                continue;
            }
        };
        if validate_save_transaction(&transaction).is_err() {
            let _ = fs::remove_file(&journal);
            continue;
        }
        let temp = transaction_temp_path(directory, &transaction);
        let target = directory.join(&transaction.target_file_name);
        if direct_file_name(&target).ok().as_deref() != Some(transaction.target_file_name.as_str())
        {
            let _ = fs::remove_file(&journal);
            continue;
        }
        match content_matches_transaction(&target, &transaction) {
            Ok(true) => {}
            Ok(false) => {
                if !remove_transaction_temp(temp.as_deref()) {
                    continue;
                }
                let _ = fs::remove_file(&journal);
                continue;
            }
            Err(error) => {
                eprintln!(
                    "Deferred recovery of managed save {}: {error}",
                    target.display()
                );
                continue;
            }
        }
        if let Err(error) =
            persist_managed_document(catalog, &target, &transaction.file_name, None, None, false)
        {
            eprintln!(
                "Deferred recovery of managed save metadata {}: {error}",
                target.display()
            );
            continue;
        }
        clear_persistent_marker(&target);
        if !remove_transaction_temp(temp.as_deref()) {
            continue;
        }
        if let Err(error) = remove_file_if_present(&journal, "managed save transaction") {
            eprintln!(
                "Failed to clear recovered managed save transaction {}: {error}",
                journal.display()
            );
        }
    }
    Ok(())
}

pub(crate) fn validate_managed_save(
    catalog: &ManagedDocumentCatalog,
    target: &Path,
    future_size: u64,
) -> Result<(), AppError> {
    let _guard = transaction_lock(catalog)?;
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

pub(crate) fn recover_completed_save(
    catalog: &ManagedDocumentCatalog,
    target: &Path,
) -> Result<(), AppError> {
    let file_name = direct_file_name(target)?;
    persist_managed_document(catalog, target, &file_name, None, None, true)?;
    clear_persistent_marker(target);
    Ok(())
}

pub(crate) fn migrate_existing_document(
    catalog: &ManagedDocumentCatalog,
    target: &Path,
    file_name: &str,
    id: &str,
    adopted_at_millis: i64,
) -> Result<(), AppError> {
    if marker_path(target)?.exists() || !target.exists() {
        return Ok(());
    }
    persist_managed_document(
        catalog,
        target,
        file_name,
        Some(id),
        Some(adopted_at_millis),
        false,
    )
}

pub(crate) fn managed_documents(
    catalog: &ManagedDocumentCatalog,
    directory: &Path,
) -> Result<Vec<ManagedDocumentRecord>, AppError> {
    let _guard = transaction_lock(catalog)?;
    scan_managed_documents(directory, true)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn reconcile_managed_documents(
    catalog: &ManagedDocumentCatalog,
    directory: &Path,
) -> Result<(), AppError> {
    managed_documents(catalog, directory).map(|_| ())
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn clear_managed_document(
    catalog: &ManagedDocumentCatalog,
    target: &Path,
) -> Result<(), AppError> {
    let _guard = transaction_lock(catalog)?;
    match fs::remove_file(marker_path(target)?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::WriteError(format!(
            "Failed to remove managed document marker: {error}"
        ))),
    }
}

fn persist_managed_document(
    catalog: &ManagedDocumentCatalog,
    target: &Path,
    file_name: &str,
    requested_id: Option<&str>,
    adopted_at_millis: Option<i64>,
    enforce_capacity: bool,
) -> Result<(), AppError> {
    let _guard = transaction_lock(catalog)?;
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
        if name.starts_with(SAVE_TRANSACTION_PREFIX) {
            continue;
        }
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

fn save_transaction_path(target: &Path) -> Result<PathBuf, AppError> {
    let parent = target.parent().ok_or_else(|| {
        AppError::DocumentStateInvalid("managed save path has no parent".to_string())
    })?;
    let file_name = direct_file_name(target)?;
    Ok(parent.join(format!(
        "{SAVE_TRANSACTION_PREFIX}{}{SAVE_TRANSACTION_SUFFIX}",
        sha256_hex(file_name.as_bytes())
    )))
}

fn read_save_transaction(path: &Path) -> Result<PersistentManagedSaveTransaction, AppError> {
    let bytes = read_marker_bytes(path, "managed save transaction")?;
    serde_json::from_slice(&bytes)
        .map_err(|error| AppError::ReadError(format!("Invalid managed save transaction: {error}")))
}

fn validate_save_transaction(
    transaction: &PersistentManagedSaveTransaction,
) -> Result<(), AppError> {
    validate_marker_field("target file name", &transaction.target_file_name)?;
    validate_marker_field("file name", &transaction.file_name)?;
    if let Some(temp_file_name) = &transaction.temp_file_name {
        validate_marker_field("temporary file name", temp_file_name)?;
        if !is_owned_temp_file_name(temp_file_name) {
            return Err(AppError::DocumentStateInvalid(
                "managed save transaction has an invalid temporary file name".to_string(),
            ));
        }
    }
    if transaction.content_sha256.len() != 64
        || !transaction
            .content_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(AppError::DocumentStateInvalid(
            "managed save transaction has an invalid content digest".to_string(),
        ));
    }
    Ok(())
}

fn validate_transaction_temp_path(
    target: &Path,
    temp: &Path,
    temp_file_name: &str,
) -> Result<(), AppError> {
    validate_marker_field("temporary file name", temp_file_name)?;
    if target.parent() != temp.parent() || !is_owned_temp_file_name(temp_file_name) {
        return Err(AppError::DocumentStateInvalid(
            "managed save temporary file must be an owned file beside its target".to_string(),
        ));
    }
    Ok(())
}

fn transaction_temp_path(
    directory: &Path,
    transaction: &PersistentManagedSaveTransaction,
) -> Option<PathBuf> {
    transaction
        .temp_file_name
        .as_ref()
        .filter(|name| is_owned_temp_file_name(name))
        .map(|name| directory.join(name))
}

fn remove_transaction_temp(temp: Option<&Path>) -> bool {
    let Some(temp) = temp else {
        return true;
    };
    if let Err(error) = remove_file_if_present(temp, "managed save temporary file") {
        eprintln!(
            "Deferred cleanup of managed save temporary file {}: {error}",
            temp.display()
        );
        return false;
    }
    true
}

fn content_matches_transaction(
    target: &Path,
    transaction: &PersistentManagedSaveTransaction,
) -> Result<bool, AppError> {
    let metadata = match fs::metadata(target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(AppError::ReadError(format!(
                "Failed to inspect managed save target: {error}"
            )));
        }
    };
    if !metadata.is_file() || metadata.len() != transaction.expected_size {
        return Ok(false);
    }
    let mut file = fs::File::open(target).map_err(|error| {
        AppError::ReadError(format!("Failed to open managed save target: {error}"))
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            AppError::ReadError(format!("Failed to verify managed save target: {error}"))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format_digest(hasher.finalize()) == transaction.content_sha256)
}

fn sha256_hex(bytes: &[u8]) -> String {
    format_digest(Sha256::digest(bytes))
}

fn format_digest(digest: impl AsRef<[u8]>) -> String {
    let digest = digest.as_ref();
    let mut digest_hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(digest_hex, "{byte:02x}");
    }
    digest_hex
}

fn remove_file_if_present(path: &Path, label: &str) -> Result<(), AppError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::WriteError(format!(
            "Failed to remove {label}: {error}"
        ))),
    }
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

fn transaction_lock(
    catalog: &ManagedDocumentCatalog,
) -> Result<std::sync::MutexGuard<'_, ()>, AppError> {
    catalog
        .transaction
        .lock()
        .map_err(|_| AppError::poisoned_lock("managed document catalog"))
}

impl ManagedDocumentCatalog {
    #[cfg(test)]
    pub(crate) fn is_same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.transaction, &other.transaction)
    }
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
    use crate::io::atomic_file::temp_path_for_target;

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

    fn begin_test_save_transaction(
        catalog: &ManagedDocumentCatalog,
        target: &Path,
        file_name: &str,
        content: &[u8],
    ) -> ManagedSaveTransaction {
        let temp = temp_path_for_target(target);
        begin_managed_save_transaction(catalog, target, &temp, file_name, content)
            .expect("save transaction")
    }

    #[test]
    fn managed_marker_recovers_document_metadata_without_recent_store() {
        let catalog = ManagedDocumentCatalog::default();
        let dir = TestDir::new("recover");
        let target = dir.0.join("document.xlsx");
        fs::write(&target, b"workbook").expect("managed file");

        persist_managed_document(&catalog, &target, "Budget.xlsx", None, Some(42), true)
            .expect("managed marker");
        let records = managed_documents(&catalog, &dir.0).expect("managed records");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].path, target);
        assert_eq!(records[0].file_name, "Budget.xlsx");
        assert_eq!(records[0].adopted_at_millis, 42);
        assert_eq!(records[0].file_size, 8);
    }

    #[test]
    fn reconciliation_removes_marker_for_missing_document() {
        let catalog = ManagedDocumentCatalog::default();
        let dir = TestDir::new("missing");
        let target = dir.0.join("missing.xlsx");
        fs::write(&target, b"workbook").expect("managed file");
        persist_managed_document(&catalog, &target, "Missing.xlsx", None, None, true)
            .expect("managed marker");
        fs::remove_file(&target).expect("remove target");

        reconcile_managed_documents(&catalog, &dir.0).expect("reconcile catalog");

        assert!(!marker_path(&target).expect("marker path").exists());
    }

    #[test]
    fn migration_preserves_existing_recent_identity() {
        let catalog = ManagedDocumentCatalog::default();
        let dir = TestDir::new("migration");
        let target = dir.0.join("legacy.xlsx");
        fs::write(&target, b"workbook").expect("managed file");

        migrate_existing_document(&catalog, &target, "Legacy.xlsx", "stable-id", 7)
            .expect("migrate document");
        let records = managed_documents(&catalog, &dir.0).expect("managed records");

        assert_eq!(records[0].id, "stable-id");
        assert_eq!(records[0].adopted_at_millis, 7);
    }

    #[test]
    fn active_adoption_retains_exclusive_transient_ownership_until_rollback() {
        let catalog = ManagedDocumentCatalog::default();
        let transient_files = Arc::new(TransientFileRegistry::default());
        let dir = TestDir::new("adoption");
        let target = dir.0.join("imported.xlsx");
        fs::write(&target, b"workbook").expect("transient file");
        transient_files
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

        let adoption = begin_transient_document_adoption(
            &catalog,
            Arc::clone(&transient_files),
            &target,
            "Imported.xlsx",
        )
        .expect("managed adoption");
        assert!(transient_files.contains(&target).unwrap());
        assert!(
            transient_files
                .begin_cleanup_if_unowned(&target)
                .unwrap()
                .is_none()
        );
        assert_eq!(managed_documents(&catalog, &dir.0).unwrap().len(), 1);

        drop(adoption);

        assert!(transient_files.contains(&target).unwrap());
        assert!(crate::io::transient_files::persistent_marker_exists_for_test(&target));
        assert!(managed_documents(&catalog, &dir.0).unwrap().is_empty());
    }

    #[test]
    fn committed_adoption_keeps_managed_ownership_and_clears_transient_marker() {
        let catalog = ManagedDocumentCatalog::default();
        let transient_files = Arc::new(TransientFileRegistry::default());
        let dir = TestDir::new("adoption-commit");
        let target = dir.0.join("imported.xlsx");
        fs::write(&target, b"workbook").expect("transient file");
        transient_files
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

        begin_transient_document_adoption(
            &catalog,
            Arc::clone(&transient_files),
            &target,
            "Imported.xlsx",
        )
        .expect("managed adoption")
        .commit();

        assert!(!transient_files.contains(&target).unwrap());
        assert!(!crate::io::transient_files::persistent_marker_exists_for_test(&target));
        assert_eq!(managed_documents(&catalog, &dir.0).unwrap().len(), 1);
    }

    #[test]
    fn adoption_rejects_a_managed_source_deleted_after_preparation() {
        let catalog = ManagedDocumentCatalog::default();
        let transient_files = Arc::new(TransientFileRegistry::default());
        let dir = TestDir::new("adoption-missing-source");
        let target = dir.0.join("deleted.xlsx");

        assert!(matches!(
            begin_transient_document_adoption(&catalog, transient_files, &target, "Deleted.xlsx",),
            Err(AppError::FileNotFound(_))
        ));
    }

    #[test]
    fn adoption_rejects_an_oversized_rollback_marker_and_restores_transient_ownership() {
        let catalog = ManagedDocumentCatalog::default();
        let transient_files = Arc::new(TransientFileRegistry::default());
        let dir = TestDir::new("adoption-oversized-marker");
        let target = dir.0.join("selected.xlsx");
        fs::write(&target, b"workbook").expect("transient file");
        transient_files
            .register(
                target.clone(),
                crate::io::transient_files::TransientFilePurpose::OpenSelection,
            )
            .expect("transient registration");
        fs::write(
            marker_path(&target).expect("managed marker path"),
            vec![b'x'; crate::io::marker_store::MAX_MARKER_BYTES + 1],
        )
        .expect("oversized managed marker");

        assert!(matches!(
            begin_transient_document_adoption(
                &catalog,
                Arc::clone(&transient_files),
                &target,
                "Selected.xlsx",
            ),
            Err(AppError::ResourceLimitExceeded(_))
        ));
        assert!(transient_files.contains(&target).unwrap());
    }

    #[test]
    fn dropped_managed_save_transaction_removes_its_recovery_journal() {
        let catalog = ManagedDocumentCatalog::default();
        let dir = TestDir::new("save-transaction-drop");
        let target = dir.0.join("reserved.xlsx");
        fs::write(&target, []).expect("reserved target");

        let transaction =
            begin_test_save_transaction(&catalog, &target, "Reserved.xlsx", b"workbook");
        let journal = save_transaction_path(&target).expect("journal path");
        assert!(journal.exists());

        drop(transaction);

        assert!(!journal.exists());
        assert!(managed_documents(&catalog, &dir.0).unwrap().is_empty());
    }

    #[test]
    fn completed_managed_save_adopts_transient_target() {
        let catalog = ManagedDocumentCatalog::default();
        let transient_files = Arc::new(TransientFileRegistry::default());
        let dir = TestDir::new("save-transaction-commit");
        let target = dir.0.join("reserved.xlsx");
        fs::write(&target, []).expect("reserved target");
        transient_files
            .register(
                target.clone(),
                crate::io::transient_files::TransientFilePurpose::SaveLocation,
            )
            .expect("transient registration");
        crate::io::transient_files::write_persistent_marker(
            &target,
            crate::io::transient_files::TransientFilePurpose::SaveLocation,
        )
        .expect("transient marker");
        let reservation = transient_files
            .reserve_if_registered(
                &target,
                crate::io::transient_files::TransientFilePurpose::SaveLocation,
            )
            .expect("reserve transient target")
            .expect("registered target");
        let mut transaction =
            begin_test_save_transaction(&catalog, &target, "Reserved.xlsx", b"workbook");
        transaction.attach_transient_reservation(reservation);

        fs::write(&target, b"workbook").expect("committed content");
        transaction
            .finish_after_content_commit()
            .expect("finish transaction");

        assert!(!transient_files.contains(&target).unwrap());
        assert!(!crate::io::transient_files::persistent_marker_exists_for_test(&target));
        assert!(!save_transaction_path(&target).unwrap().exists());
        let records = managed_documents(&catalog, &dir.0).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].file_name, "Reserved.xlsx");
    }

    #[test]
    fn committed_content_loses_transient_identity_even_if_metadata_persistence_fails() {
        let catalog = ManagedDocumentCatalog::default();
        let transient_files = Arc::new(TransientFileRegistry::default());
        let dir = TestDir::new("save-metadata-failure");
        let target = dir.0.join("reserved.xlsx");
        fs::write(&target, []).expect("reserved target");
        transient_files
            .register(
                target.clone(),
                crate::io::transient_files::TransientFilePurpose::SaveLocation,
            )
            .expect("transient registration");
        crate::io::transient_files::write_persistent_marker(
            &target,
            crate::io::transient_files::TransientFilePurpose::SaveLocation,
        )
        .expect("transient marker");
        let reservation = transient_files
            .reserve_if_registered(
                &target,
                crate::io::transient_files::TransientFilePurpose::SaveLocation,
            )
            .expect("reserve transient target")
            .expect("registered target");
        let mut transaction =
            begin_test_save_transaction(&catalog, &target, "Reserved.xlsx", b"workbook");
        transaction.attach_transient_reservation(reservation);
        let journal = save_transaction_path(&target).expect("journal path");

        fs::remove_file(&target).expect("inject metadata failure");
        assert!(transaction.finish_after_content_commit().is_err());

        assert!(!transient_files.contains(&target).unwrap());
        assert!(!crate::io::transient_files::persistent_marker_exists_for_test(&target));
        assert!(journal.exists());
    }

    #[test]
    fn recovery_finishes_only_transactions_whose_content_was_committed() {
        let catalog = ManagedDocumentCatalog::default();
        let dir = TestDir::new("save-transaction-recovery");
        let completed = dir.0.join("completed.xlsx");
        let incomplete = dir.0.join("incomplete.xlsx");
        fs::write(&completed, []).expect("completed reservation");
        fs::write(&incomplete, b"old-data").expect("incomplete target");

        let completed_temp = temp_path_for_target(&completed);
        let incomplete_temp = temp_path_for_target(&incomplete);
        let completed_transaction = begin_managed_save_transaction(
            &catalog,
            &completed,
            &completed_temp,
            "Completed.xlsx",
            b"new-data",
        )
        .expect("completed transaction");
        let incomplete_transaction = begin_managed_save_transaction(
            &catalog,
            &incomplete,
            &incomplete_temp,
            "Incomplete.xlsx",
            b"different-data",
        )
        .expect("incomplete transaction");
        fs::write(&completed_temp, b"staged-completed").expect("completed temp");
        fs::write(&incomplete_temp, b"staged-incomplete").expect("incomplete temp");
        fs::write(&completed, b"new-data").expect("committed content");
        std::mem::forget(completed_transaction);
        std::mem::forget(incomplete_transaction);

        recover_managed_save_transactions(&catalog, &dir.0).expect("recover transactions");

        let records = managed_documents(&catalog, &dir.0).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].path, completed);
        assert!(!save_transaction_path(&completed).unwrap().exists());
        assert!(!save_transaction_path(&incomplete).unwrap().exists());
        assert!(!completed_temp.exists());
        assert!(!incomplete_temp.exists());
    }

    #[test]
    fn recovery_accepts_legacy_save_transaction_without_temp_file_name() {
        let catalog = ManagedDocumentCatalog::default();
        let dir = TestDir::new("legacy-save-transaction");
        let target = dir.0.join("legacy.xlsx");
        fs::write(&target, b"committed").expect("committed target");
        let journal = save_transaction_path(&target).expect("journal path");
        let legacy = serde_json::json!({
            "targetFileName": "legacy.xlsx",
            "fileName": "Legacy.xlsx",
            "expectedSize": 9,
            "contentSha256": sha256_hex(b"committed")
        });
        fs::write(&journal, serde_json::to_vec(&legacy).unwrap()).expect("legacy journal");

        recover_managed_save_transactions(&catalog, &dir.0).expect("recover legacy transaction");

        assert!(!journal.exists());
        let records = managed_documents(&catalog, &dir.0).expect("managed documents");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].path, target);
    }

    #[test]
    fn new_managed_documents_are_rejected_at_the_catalog_count_limit() {
        let catalog = ManagedDocumentCatalog::default();
        let dir = TestDir::new("capacity");
        for index in 0..MAX_MANAGED_DOCUMENTS {
            let target = dir.0.join(format!("document-{index}.xlsx"));
            fs::write(&target, b"workbook").expect("managed file");
            persist_managed_document(&catalog, &target, "Document.xlsx", None, None, true)
                .expect("managed marker");
        }
        let overflow = dir.0.join("overflow.xlsx");
        fs::write(&overflow, b"workbook").expect("overflow file");

        assert!(matches!(
            persist_managed_document(&catalog, &overflow, "Overflow.xlsx", None, None, true),
            Err(AppError::ResourceLimitExceeded(_))
        ));
        assert_eq!(
            managed_documents(&catalog, &dir.0).unwrap().len(),
            MAX_MANAGED_DOCUMENTS
        );
    }

    #[test]
    fn save_capacity_allows_existing_replacement_but_rejects_oversized_new_file() {
        let catalog = ManagedDocumentCatalog::default();
        let dir = TestDir::new("save-capacity");
        let target = dir.0.join("existing.xlsx");
        fs::write(&target, b"workbook").expect("managed file");
        migrate_existing_document(&catalog, &target, "Existing.xlsx", "existing", 1)
            .expect("managed marker");

        assert!(validate_managed_save(&catalog, &target, 1).is_ok());
        assert!(matches!(
            validate_managed_save(
                &catalog,
                &dir.0.join("new.xlsx"),
                MAX_MANAGED_DOCUMENT_BYTES + 1
            ),
            Err(AppError::ResourceLimitExceeded(_))
        ));
    }
}
