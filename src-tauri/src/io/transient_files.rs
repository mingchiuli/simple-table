#![cfg_attr(test, allow(dead_code))]

use crate::error::AppError;
use crate::io::atomic_file::write_file_atomically;
use crate::io::marker_store::{
    bounded_directory_entries, read_marker_bytes, validate_marker_field,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_TRANSIENT_FILES_PER_PURPOSE: usize = 64;
const TRANSIENT_FILE_TTL: Duration = Duration::from_secs(30 * 60);
const MARKER_PREFIX: &str = ".simple-table-transient-";
const MARKER_SUFFIX: &str = ".json";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TransientFilePurpose {
    OpenSelection,
    SaveLocation,
}

#[derive(Clone, Copy)]
struct TransientFileEntry {
    purpose: TransientFilePurpose,
    created_at: Instant,
}

#[derive(Clone, Copy)]
enum TransientFileState {
    Registered(TransientFileEntry),
    Reserved {
        entry: TransientFileEntry,
        reservation_id: uuid::Uuid,
    },
    Cleanup {
        reservation_id: uuid::Uuid,
    },
}

impl TransientFileState {
    fn purpose(self) -> Option<TransientFilePurpose> {
        match self {
            Self::Registered(entry) | Self::Reserved { entry, .. } => Some(entry.purpose),
            Self::Cleanup { .. } => None,
        }
    }
}

pub(crate) struct TransientFileReservation {
    registry: Arc<TransientFileRegistry>,
    target: PathBuf,
    reservation_id: uuid::Uuid,
    active: bool,
}

pub(crate) struct TransientPathCleanupLease {
    registry: Arc<TransientFileRegistry>,
    target: PathBuf,
    reservation_id: uuid::Uuid,
}

impl TransientFileReservation {
    pub(crate) fn commit(mut self) {
        if self
            .registry
            .commit_reservation(&self.target, self.reservation_id)
            .unwrap_or(false)
        {
            self.active = false;
            clear_persistent_marker(&self.target);
        }
    }
}

impl Drop for TransientFileReservation {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        if self
            .registry
            .release_reservation(&self.target, self.reservation_id)
            .unwrap_or(false)
        {
            self.active = false;
        }
    }
}

impl Drop for TransientPathCleanupLease {
    fn drop(&mut self) {
        let _ = self
            .registry
            .release_cleanup(&self.target, self.reservation_id);
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistentTransientMarker {
    target_file_name: String,
    purpose: TransientFilePurpose,
    created_at_millis: u64,
}

#[derive(Default)]
pub struct TransientFileRegistry {
    paths: Mutex<HashMap<PathBuf, TransientFileState>>,
}

impl TransientFileRegistry {
    pub fn register(&self, path: PathBuf, purpose: TransientFilePurpose) -> Result<(), AppError> {
        self.register_at(path, purpose, Instant::now())
    }

    fn register_at(
        &self,
        path: PathBuf,
        purpose: TransientFilePurpose,
        now: Instant,
    ) -> Result<(), AppError> {
        let (expired, result) = {
            let mut paths = self
                .paths
                .lock()
                .map_err(|_| AppError::poisoned_lock("transient file registry"))?;
            let expired = prune_expired(&mut paths, now);
            let result = if let Some(existing) = paths.get_mut(&path) {
                match existing {
                    TransientFileState::Registered(entry) if entry.purpose == purpose => {
                        entry.created_at = now;
                        Ok(())
                    }
                    TransientFileState::Registered(_) => Err(AppError::DocumentStateInvalid(
                        "transient file is already registered for a different purpose".to_string(),
                    )),
                    TransientFileState::Reserved { .. } | TransientFileState::Cleanup { .. } => {
                        Err(AppError::DocumentStateInvalid(
                            "transient file path is already in use".to_string(),
                        ))
                    }
                }
            } else if paths
                .values()
                .filter(|state| state.purpose() == Some(purpose))
                .count()
                >= MAX_TRANSIENT_FILES_PER_PURPOSE
            {
                Err(AppError::ResourceLimitExceeded(format!(
                    "at most {MAX_TRANSIENT_FILES_PER_PURPOSE} transient files may be registered for {purpose:?}"
                )))
            } else {
                paths.insert(
                    path.clone(),
                    TransientFileState::Registered(TransientFileEntry {
                        purpose,
                        created_at: now,
                    }),
                );
                Ok(())
            };
            (expired, result)
        };

        self.cleanup_expired(expired);
        result
    }

    pub fn take(&self, path: &Path, purpose: TransientFilePurpose) -> Result<PathBuf, AppError> {
        self.take_at(path, purpose, Instant::now())
    }

    fn take_at(
        &self,
        path: &Path,
        purpose: TransientFilePurpose,
        now: Instant,
    ) -> Result<PathBuf, AppError> {
        let target = path.to_path_buf();
        let (expired, result) = {
            let mut paths = self
                .paths
                .lock()
                .map_err(|_| AppError::poisoned_lock("transient file registry"))?;
            let expired = prune_expired(&mut paths, now);
            let result = match paths.get(&target).copied() {
                Some(TransientFileState::Registered(entry)) if entry.purpose == purpose => {
                    paths.remove(&target);
                    Ok(target.clone())
                }
                _ => Err(AppError::DocumentStateInvalid(
                    "Refusing to discard a file that is not available for this purpose".to_string(),
                )),
            };
            (expired, result)
        };
        self.cleanup_expired(expired);
        result
    }

    pub fn adopt_if_registered(&self, path: &Path) -> Result<bool, AppError> {
        self.adopt_at(path, Instant::now())
    }

    pub(crate) fn reserve_if_registered(
        self: &Arc<Self>,
        path: &Path,
        purpose: TransientFilePurpose,
    ) -> Result<Option<TransientFileReservation>, AppError> {
        let target = path.to_path_buf();
        let reservation_id = uuid::Uuid::new_v4();
        let (expired, reserved) = {
            let mut paths = self
                .paths
                .lock()
                .map_err(|_| AppError::poisoned_lock("transient file registry"))?;
            let expired = prune_expired(&mut paths, Instant::now());
            let reserved = match paths.get(&target).copied() {
                Some(TransientFileState::Registered(entry)) if entry.purpose == purpose => {
                    paths.insert(
                        target.clone(),
                        TransientFileState::Reserved {
                            entry,
                            reservation_id,
                        },
                    );
                    Ok(true)
                }
                Some(TransientFileState::Registered(_)) => Err(AppError::DocumentStateInvalid(
                    "transient file is registered for a different purpose".to_string(),
                )),
                Some(TransientFileState::Reserved { .. })
                | Some(TransientFileState::Cleanup { .. }) => Err(AppError::DocumentStateInvalid(
                    "transient file path is already in use".to_string(),
                )),
                None => Ok(false),
            };
            (expired, reserved)
        };
        self.cleanup_expired(expired);
        Ok(reserved?.then(|| TransientFileReservation {
            registry: Arc::clone(self),
            target,
            reservation_id,
            active: true,
        }))
    }

    pub(crate) fn begin_cleanup_if_unowned(
        self: &Arc<Self>,
        path: &Path,
    ) -> Result<Option<TransientPathCleanupLease>, AppError> {
        let target = path.to_path_buf();
        let reservation_id = uuid::Uuid::new_v4();
        let (expired, acquired) = {
            let mut paths = self
                .paths
                .lock()
                .map_err(|_| AppError::poisoned_lock("transient file registry"))?;
            let expired = prune_expired(&mut paths, Instant::now());
            let acquired = if paths.contains_key(&target) {
                false
            } else {
                paths.insert(
                    target.clone(),
                    TransientFileState::Cleanup { reservation_id },
                );
                true
            };
            (expired, acquired)
        };
        self.cleanup_expired(expired);
        Ok(acquired.then(|| TransientPathCleanupLease {
            registry: Arc::clone(self),
            target,
            reservation_id,
        }))
    }

    fn commit_reservation(
        &self,
        path: &Path,
        reservation_id: uuid::Uuid,
    ) -> Result<bool, AppError> {
        let mut paths = self
            .paths
            .lock()
            .map_err(|_| AppError::poisoned_lock("transient file registry"))?;
        if matches!(
            paths.get(path),
            Some(TransientFileState::Reserved {
                reservation_id: current,
                ..
            }) if *current == reservation_id
        ) {
            paths.remove(path);
            return Ok(true);
        }
        Ok(false)
    }

    fn release_reservation(
        &self,
        path: &Path,
        reservation_id: uuid::Uuid,
    ) -> Result<bool, AppError> {
        let mut paths = self
            .paths
            .lock()
            .map_err(|_| AppError::poisoned_lock("transient file registry"))?;
        let Some(TransientFileState::Reserved {
            entry,
            reservation_id: current,
        }) = paths.get(path).copied()
        else {
            return Ok(false);
        };
        if current != reservation_id {
            return Ok(false);
        }
        paths.insert(path.to_path_buf(), TransientFileState::Registered(entry));
        Ok(true)
    }

    fn release_cleanup(&self, path: &Path, reservation_id: uuid::Uuid) -> Result<(), AppError> {
        let mut paths = self
            .paths
            .lock()
            .map_err(|_| AppError::poisoned_lock("transient file registry"))?;
        if matches!(
            paths.get(path),
            Some(TransientFileState::Cleanup {
                reservation_id: current,
            }) if *current == reservation_id
        ) {
            paths.remove(path);
        }
        Ok(())
    }

    fn adopt_at(&self, path: &Path, now: Instant) -> Result<bool, AppError> {
        let (expired, adopted) = {
            let mut paths = self
                .paths
                .lock()
                .map_err(|_| AppError::poisoned_lock("transient file registry"))?;
            let expired = prune_expired(&mut paths, now);
            let adopted = matches!(paths.get(path), Some(TransientFileState::Registered(_)));
            if adopted {
                paths.remove(path);
            }
            (expired, adopted)
        };
        self.cleanup_expired(expired);
        if adopted {
            clear_persistent_marker(path);
        }
        Ok(adopted)
    }

    pub fn contains(&self, path: &Path) -> Result<bool, AppError> {
        self.contains_at(path, None, Instant::now())
    }

    #[cfg(test)]
    pub fn contains_for(
        &self,
        path: &Path,
        purpose: TransientFilePurpose,
    ) -> Result<bool, AppError> {
        self.contains_at(path, Some(purpose), Instant::now())
    }

    fn contains_at(
        &self,
        path: &Path,
        purpose: Option<TransientFilePurpose>,
        now: Instant,
    ) -> Result<bool, AppError> {
        let (expired, contains) = {
            let mut paths = self
                .paths
                .lock()
                .map_err(|_| AppError::poisoned_lock("transient file registry"))?;
            let expired = prune_expired(&mut paths, now);
            let contains = paths.get(path).is_some_and(|state| {
                purpose.is_none_or(|purpose| state.purpose() == Some(purpose))
            });
            (expired, contains)
        };
        self.cleanup_expired(expired);
        Ok(contains)
    }

    fn cleanup_expired(&self, paths: Vec<PathBuf>) {
        for path in paths {
            let Ok(registered) = self.paths.lock() else {
                return;
            };
            if registered.contains_key(&path) {
                continue;
            }
            cleanup_transient_artifacts(&path);
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.paths.lock().expect("registry lock").len()
    }
}

pub(crate) fn write_persistent_marker(
    target: &Path,
    purpose: TransientFilePurpose,
) -> Result<(), AppError> {
    write_persistent_marker_at(target, purpose, SystemTime::now())
}

fn write_persistent_marker_at(
    target: &Path,
    purpose: TransientFilePurpose,
    created_at: SystemTime,
) -> Result<(), AppError> {
    let target_file_name = direct_file_name(target)?;
    validate_marker_field("target file name", &target_file_name)?;
    let marker = PersistentTransientMarker {
        target_file_name,
        purpose,
        created_at_millis: system_time_millis(created_at),
    };
    let bytes = serde_json::to_vec(&marker).map_err(|error| {
        AppError::Internal(format!("Failed to encode transient marker: {error}"))
    })?;
    write_file_atomically(&marker_path(target)?, &bytes)
}

pub(crate) fn reconcile_persisted_transient_files(directory: &Path) -> Result<(), AppError> {
    let entries = bounded_directory_entries(directory, "transient file")?;
    let now_millis = system_time_millis(SystemTime::now());
    let ttl_millis = TRANSIENT_FILE_TTL.as_millis().min(u64::MAX as u128) as u64;

    for entry in entries {
        let marker_path = entry.path();
        let Some(name) = marker_path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with(MARKER_PREFIX) || !name.ends_with(MARKER_SUFFIX) {
            continue;
        }
        let bytes = match read_marker_bytes(&marker_path, "transient file marker") {
            Ok(bytes) => bytes,
            Err(AppError::ResourceLimitExceeded(_)) => {
                let _ = fs::remove_file(&marker_path);
                continue;
            }
            Err(_) => continue,
        };
        let Ok(marker) = serde_json::from_slice::<PersistentTransientMarker>(&bytes) else {
            let _ = fs::remove_file(&marker_path);
            continue;
        };
        if validate_marker_field("target file name", &marker.target_file_name).is_err() {
            let _ = fs::remove_file(&marker_path);
            continue;
        }
        if now_millis.saturating_sub(marker.created_at_millis) < ttl_millis {
            continue;
        }
        let target = directory.join(&marker.target_file_name);
        if direct_file_name(&target).ok().as_deref() == Some(marker.target_file_name.as_str()) {
            let _ = fs::remove_file(target);
        }
        let _ = fs::remove_file(marker_path);
    }
    Ok(())
}

pub(crate) fn completed_persisted_save_locations(
    directory: &Path,
) -> Result<Vec<PathBuf>, AppError> {
    let entries = bounded_directory_entries(directory, "transient file")?;
    let mut completed = Vec::new();
    for entry in entries {
        let marker_path = entry.path();
        let Some(name) = marker_path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with(MARKER_PREFIX) || !name.ends_with(MARKER_SUFFIX) {
            continue;
        }
        let Some(marker) = read_marker_bytes(&marker_path, "transient file marker")
            .ok()
            .and_then(|bytes| serde_json::from_slice::<PersistentTransientMarker>(&bytes).ok())
            .filter(|marker| {
                validate_marker_field("target file name", &marker.target_file_name).is_ok()
            })
        else {
            continue;
        };
        if marker.purpose != TransientFilePurpose::SaveLocation {
            continue;
        }
        let target = directory.join(marker.target_file_name);
        if fs::metadata(&target).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0) {
            completed.push(target);
        }
    }
    Ok(completed)
}

fn prune_expired(paths: &mut HashMap<PathBuf, TransientFileState>, now: Instant) -> Vec<PathBuf> {
    let mut expired = Vec::new();
    paths.retain(|path, state| {
        let keep = match state {
            TransientFileState::Registered(entry) => {
                now.saturating_duration_since(entry.created_at) < TRANSIENT_FILE_TTL
            }
            TransientFileState::Reserved { .. } | TransientFileState::Cleanup { .. } => true,
        };
        if !keep {
            expired.push(path.clone());
        }
        keep
    });
    expired
}

fn cleanup_transient_artifacts(target: &Path) {
    let _ = fs::remove_file(target);
    clear_persistent_marker(target);
}

pub(crate) fn clear_persistent_marker(target: &Path) {
    if let Ok(marker) = marker_path(target) {
        let _ = fs::remove_file(marker);
    }
}

#[cfg(test)]
pub(crate) fn persistent_marker_exists_for_test(target: &Path) -> bool {
    marker_path(target).is_ok_and(|path| path.exists())
}

fn marker_path(target: &Path) -> Result<PathBuf, AppError> {
    let parent = target.parent().ok_or_else(|| {
        AppError::DocumentStateInvalid("transient file path has no parent directory".to_string())
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
            AppError::DocumentStateInvalid("transient file path has no valid file name".to_string())
        })
}

fn system_time_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::marker_store::MAX_MARKER_BYTES;

    #[test]
    fn registered_path_can_be_taken_once_for_its_purpose() {
        let registry = TransientFileRegistry::default();
        let path = PathBuf::from("tmp").join("imported.xlsx");
        registry
            .register(path.clone(), TransientFilePurpose::OpenSelection)
            .unwrap();

        assert_eq!(
            registry
                .take(&path, TransientFilePurpose::OpenSelection)
                .unwrap(),
            path
        );
        assert!(
            registry
                .take(&path, TransientFilePurpose::OpenSelection)
                .is_err()
        );
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn taking_a_file_keeps_its_marker_until_disk_deletion_finishes() {
        let directory = TestDir::new("take-marker");
        let path = directory.path.join("discarded.xlsx");
        fs::write(&path, b"temporary").unwrap();
        let registry = TransientFileRegistry::default();
        registry
            .register(path.clone(), TransientFilePurpose::OpenSelection)
            .unwrap();
        write_persistent_marker(&path, TransientFilePurpose::OpenSelection).unwrap();

        registry
            .take(&path, TransientFilePurpose::OpenSelection)
            .unwrap();

        assert!(marker_path(&path).unwrap().exists());
    }

    #[test]
    fn adopting_registered_path_prevents_later_take() {
        let registry = TransientFileRegistry::default();
        let path = PathBuf::from("tmp").join("saved.xlsx");
        registry
            .register(path.clone(), TransientFilePurpose::SaveLocation)
            .unwrap();

        assert!(registry.adopt_if_registered(&path).unwrap());
        assert!(
            registry
                .take(&path, TransientFilePurpose::SaveLocation)
                .is_err()
        );
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn adopting_unknown_path_is_a_noop() {
        let registry = TransientFileRegistry::default();

        assert!(
            !registry
                .adopt_if_registered(&PathBuf::from("tmp").join("unknown.xlsx"))
                .unwrap()
        );
    }

    #[test]
    fn reserved_save_location_cannot_be_discarded_while_save_owns_it() {
        let registry = Arc::new(TransientFileRegistry::default());
        let path = PathBuf::from("tmp").join("saving.xlsx");
        registry
            .register(path.clone(), TransientFilePurpose::SaveLocation)
            .unwrap();

        let reservation = registry
            .reserve_if_registered(&path, TransientFilePurpose::SaveLocation)
            .unwrap()
            .expect("save reservation");

        assert!(registry.contains(&path).unwrap());
        assert!(registry.begin_cleanup_if_unowned(&path).unwrap().is_none());
        assert!(
            registry
                .take(&path, TransientFilePurpose::SaveLocation)
                .is_err()
        );
        drop(reservation);
        assert_eq!(
            registry
                .take(&path, TransientFilePurpose::SaveLocation)
                .unwrap(),
            path
        );
    }

    #[test]
    fn cleanup_lease_excludes_new_transient_ownership_until_release() {
        let registry = Arc::new(TransientFileRegistry::default());
        let path = PathBuf::from("tmp").join("cleanup.xlsx");
        let cleanup = registry
            .begin_cleanup_if_unowned(&path)
            .unwrap()
            .expect("cleanup lease");

        assert!(
            registry
                .register(path.clone(), TransientFilePurpose::OpenSelection)
                .is_err()
        );
        assert!(
            registry
                .reserve_if_registered(&path, TransientFilePurpose::OpenSelection)
                .is_err()
        );

        drop(cleanup);
        registry
            .register(path, TransientFilePurpose::OpenSelection)
            .unwrap();
    }

    #[test]
    fn active_reservation_is_not_expired_until_ownership_is_released() {
        let directory = TestDir::new("reserved-expiry");
        let path = directory.path.join("saving.xlsx");
        fs::write(&path, b"temporary").unwrap();
        let registry = Arc::new(TransientFileRegistry::default());
        let now = Instant::now();
        registry
            .register_at(path.clone(), TransientFilePurpose::SaveLocation, now)
            .unwrap();
        let reservation = registry
            .reserve_if_registered(&path, TransientFilePurpose::SaveLocation)
            .unwrap()
            .expect("save reservation");
        let after_expiry = now + TRANSIENT_FILE_TTL + Duration::from_secs(1);

        assert!(registry.contains_at(&path, None, after_expiry).unwrap());
        assert!(path.exists());

        drop(reservation);
        assert!(!registry.contains_at(&path, None, after_expiry).unwrap());
        assert!(!path.exists());
    }

    #[test]
    fn committed_save_reservation_removes_transient_ownership() {
        let directory = TestDir::new("save-reservation-commit");
        let path = directory.path.join("saved.xlsx");
        fs::write(&path, b"saved workbook").unwrap();
        let registry = Arc::new(TransientFileRegistry::default());
        registry
            .register(path.clone(), TransientFilePurpose::SaveLocation)
            .unwrap();
        write_persistent_marker(&path, TransientFilePurpose::SaveLocation).unwrap();
        let reservation = registry
            .reserve_if_registered(&path, TransientFilePurpose::SaveLocation)
            .unwrap()
            .expect("save reservation");

        reservation.commit();

        assert!(!registry.contains(&path).unwrap());
        assert!(!marker_path(&path).unwrap().exists());
        assert!(path.exists());
    }

    #[test]
    fn duplicate_registration_refreshes_the_same_purpose() {
        let registry = TransientFileRegistry::default();
        let path = PathBuf::from("tmp").join("repeated.xlsx");
        let now = Instant::now();
        registry
            .register_at(
                path.clone(),
                TransientFilePurpose::OpenSelection,
                now - TRANSIENT_FILE_TTL + Duration::from_secs(1),
            )
            .unwrap();
        registry
            .register_at(path.clone(), TransientFilePurpose::OpenSelection, now)
            .unwrap();

        assert!(
            registry
                .contains_at(&path, None, now + Duration::from_secs(2))
                .unwrap()
        );
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn registrations_are_bounded_per_purpose() {
        let registry = TransientFileRegistry::default();
        let now = Instant::now();
        for index in 0..MAX_TRANSIENT_FILES_PER_PURPOSE {
            registry
                .register_at(
                    PathBuf::from("tmp").join(format!("open-{index}.xlsx")),
                    TransientFilePurpose::OpenSelection,
                    now,
                )
                .unwrap();
            registry
                .register_at(
                    PathBuf::from("tmp").join(format!("save-{index}.xlsx")),
                    TransientFilePurpose::SaveLocation,
                    now,
                )
                .unwrap();
        }

        assert!(matches!(
            registry.register_at(
                PathBuf::from("tmp").join("one-too-many.xlsx"),
                TransientFilePurpose::OpenSelection,
                now,
            ),
            Err(AppError::ResourceLimitExceeded(_))
        ));
        assert_eq!(registry.len(), MAX_TRANSIENT_FILES_PER_PURPOSE * 2);
    }

    #[test]
    fn expired_registration_removes_the_owned_file() {
        let directory = TestDir::new("expiry");
        let path = directory.path.join("expired.xlsx");
        fs::write(&path, b"temporary").unwrap();
        let registry = TransientFileRegistry::default();
        let now = Instant::now();
        registry
            .register_at(
                path.clone(),
                TransientFilePurpose::OpenSelection,
                now - TRANSIENT_FILE_TTL,
            )
            .unwrap();

        assert!(!registry.contains_at(&path, None, now).unwrap());
        assert!(!path.exists());
    }

    #[test]
    fn expired_cleanup_preserves_a_path_registered_again_before_deletion() {
        let directory = TestDir::new("expiry-reregistered");
        let path = directory.path.join("reused.xlsx");
        fs::write(&path, b"new transient content").unwrap();
        let registry = TransientFileRegistry::default();
        let now = Instant::now();
        registry
            .register_at(
                path.clone(),
                TransientFilePurpose::OpenSelection,
                now - TRANSIENT_FILE_TTL,
            )
            .unwrap();
        let expired = {
            let mut paths = registry.paths.lock().unwrap();
            prune_expired(&mut paths, now)
        };
        registry
            .register_at(path.clone(), TransientFilePurpose::OpenSelection, now)
            .unwrap();

        registry.cleanup_expired(expired);

        assert!(path.exists());
        assert!(registry.contains_at(&path, None, now).unwrap());
    }

    #[test]
    fn persisted_marker_reconciles_an_expired_file_after_restart() {
        let directory = TestDir::new("persisted-expiry");
        let path = directory.path.join("expired.xlsx");
        fs::write(&path, b"temporary").unwrap();
        write_persistent_marker_at(
            &path,
            TransientFilePurpose::OpenSelection,
            SystemTime::now() - TRANSIENT_FILE_TTL - Duration::from_secs(1),
        )
        .unwrap();

        reconcile_persisted_transient_files(&directory.path).unwrap();

        assert!(!path.exists());
        assert!(!marker_path(&path).unwrap().exists());
    }

    #[test]
    fn persisted_marker_preserves_a_live_file() {
        let directory = TestDir::new("persisted-live");
        let path = directory.path.join("live.xlsx");
        fs::write(&path, b"temporary").unwrap();
        write_persistent_marker_at(
            &path,
            TransientFilePurpose::OpenSelection,
            SystemTime::now(),
        )
        .unwrap();

        reconcile_persisted_transient_files(&directory.path).unwrap();

        assert!(path.exists());
        assert!(marker_path(&path).unwrap().exists());
    }

    #[test]
    fn reconciliation_removes_an_oversized_persistent_marker() {
        let directory = TestDir::new("oversized-marker");
        let target = directory.path.join("temporary.xlsx");
        fs::write(&target, b"temporary").unwrap();
        let marker = marker_path(&target).unwrap();
        fs::write(&marker, vec![b'x'; MAX_MARKER_BYTES + 1]).unwrap();

        reconcile_persisted_transient_files(&directory.path).unwrap();

        assert!(!marker.exists());
        assert!(target.exists());
    }

    #[test]
    fn completed_save_locations_are_recoverable_before_transient_expiry() {
        let directory = TestDir::new("completed-save");
        let path = directory.path.join("saved.xlsx");
        fs::write(&path, b"completed workbook").unwrap();
        write_persistent_marker_at(&path, TransientFilePurpose::SaveLocation, SystemTime::now())
            .unwrap();

        assert_eq!(
            completed_persisted_save_locations(&directory.path).unwrap(),
            vec![path]
        );
    }

    #[test]
    fn empty_reserved_save_location_is_not_treated_as_completed() {
        let directory = TestDir::new("empty-save");
        let path = directory.path.join("reserved.xlsx");
        fs::write(&path, b"").unwrap();
        write_persistent_marker_at(&path, TransientFilePurpose::SaveLocation, SystemTime::now())
            .unwrap();

        assert!(
            completed_persisted_save_locations(&directory.path)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn adopting_a_registered_file_removes_its_persistent_marker() {
        let directory = TestDir::new("adopt-marker");
        let path = directory.path.join("adopted.xlsx");
        fs::write(&path, b"temporary").unwrap();
        let registry = TransientFileRegistry::default();
        registry
            .register(path.clone(), TransientFilePurpose::OpenSelection)
            .unwrap();
        write_persistent_marker(&path, TransientFilePurpose::OpenSelection).unwrap();

        assert!(registry.adopt_if_registered(&path).unwrap());
        assert!(path.exists());
        assert!(!marker_path(&path).unwrap().exists());
    }

    #[test]
    fn registration_cannot_change_a_transient_file_purpose() {
        let registry = TransientFileRegistry::default();
        let path = PathBuf::from("tmp").join("selection.xlsx");
        registry
            .register(path.clone(), TransientFilePurpose::OpenSelection)
            .unwrap();

        assert!(
            registry
                .register(path, TransientFilePurpose::SaveLocation)
                .is_err()
        );
    }

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "simple-table-transient-{label}-{}",
                uuid::Uuid::new_v4()
            ));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
