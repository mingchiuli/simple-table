use crate::error::AppError;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

const TEMP_FILE_PREFIX: &str = ".simple-table-atomic-";
const TEMP_FILE_SUFFIX: &str = ".tmp";
const MAX_STALE_TEMP_SCAN_ENTRIES: usize = 1_024;

pub(crate) enum AtomicReplaceError {
    NotReplaced(AppError),
    ReplacedNotDurable(AppError),
}

impl AtomicReplaceError {
    pub(crate) fn into_app_error(self) -> AppError {
        match self {
            Self::NotReplaced(error) | Self::ReplacedNotDurable(error) => error,
        }
    }
}

pub fn write_file_atomically(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let temp_path = write_temp_file_for_target(path, bytes)?;
    let result = replace_temp_file(&temp_path, path);
    if result.is_err() {
        cleanup_temp_file(&temp_path);
    }
    result
}

pub fn write_temp_file_for_target(target: &Path, bytes: &[u8]) -> Result<PathBuf, AppError> {
    let temp_path = temp_path_for_target(target);
    write_temp_file(&temp_path, bytes)?;
    Ok(temp_path)
}

pub fn temp_path_for_target(target: &Path) -> PathBuf {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!(
        "{TEMP_FILE_PREFIX}{}{TEMP_FILE_SUFFIX}",
        uuid::Uuid::new_v4()
    ))
}

pub fn write_temp_file(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| AppError::WriteError(error.to_string()))?;
    let result = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| AppError::WriteError(error.to_string()));
    drop(file);
    if result.is_err() {
        cleanup_temp_file(path);
    }
    result
}

pub(crate) fn is_owned_temp_file_name(name: &str) -> bool {
    name.strip_prefix(TEMP_FILE_PREFIX)
        .and_then(|name| name.strip_suffix(TEMP_FILE_SUFFIX))
        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
}

#[cfg(any(target_os = "android", target_os = "ios", test))]
pub(crate) fn cleanup_orphaned_temp_files(directory: &Path) -> Result<(), AppError> {
    cleanup_owned_temp_files(directory, None, None)
}

pub(crate) fn cleanup_stale_temp_files(
    directory: &Path,
    minimum_age: Duration,
) -> Result<(), AppError> {
    cleanup_owned_temp_files(
        directory,
        Some(minimum_age),
        Some(MAX_STALE_TEMP_SCAN_ENTRIES),
    )
}

fn cleanup_owned_temp_files(
    directory: &Path,
    minimum_age: Option<Duration>,
    maximum_entries: Option<usize>,
) -> Result<(), AppError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(AppError::ReadError(format!(
                "Failed to inspect atomic temporary file directory: {error}"
            )));
        }
    };

    for entry in entries.take(maximum_entries.unwrap_or(usize::MAX)) {
        let entry = entry.map_err(|error| {
            AppError::ReadError(format!(
                "Failed to inspect atomic temporary file directory entry: {error}"
            ))
        })?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !is_owned_temp_file_name(&name) && !is_legacy_owned_temp_file_name(&name) {
            continue;
        }
        let file_type = entry.file_type().map_err(|error| {
            AppError::ReadError(format!(
                "Failed to inspect atomic temporary file {name}: {error}"
            ))
        })?;
        if !file_type.is_file() {
            continue;
        }
        if minimum_age.is_some_and(|minimum_age| {
            !entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age >= minimum_age)
        }) {
            continue;
        }
        match fs::remove_file(entry.path()) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(AppError::WriteError(format!(
                    "Failed to remove orphaned atomic temporary file {name}: {error}"
                )));
            }
        }
    }
    Ok(())
}

pub fn replace_temp_file(temp_path: &Path, target: &Path) -> Result<(), AppError> {
    replace_temp_file_detailed(temp_path, target).map_err(AtomicReplaceError::into_app_error)
}

pub(crate) fn replace_temp_file_detailed(
    temp_path: &Path,
    target: &Path,
) -> Result<(), AtomicReplaceError> {
    replace_file(temp_path, target).map_err(|error| {
        AtomicReplaceError::NotReplaced(AppError::WriteError(error.to_string()))
    })?;
    finish_replacement(sync_parent_dir(target))
}

fn finish_replacement(sync_result: std::io::Result<()>) -> Result<(), AtomicReplaceError> {
    sync_result.map_err(|error| {
        AtomicReplaceError::ReplacedNotDurable(AppError::WriteError(format!(
            "File content was replaced but its parent directory could not be synchronized: {error}"
        )))
    })
}

pub fn cleanup_temp_file(temp_path: &Path) {
    let _ = fs::remove_file(temp_path);
}

fn is_legacy_owned_temp_file_name(name: &str) -> bool {
    name.strip_prefix('.')
        .and_then(|name| name.strip_suffix(TEMP_FILE_SUFFIX))
        .and_then(|name| name.rsplit_once('.'))
        .is_some_and(|(target, id)| !target.is_empty() && uuid::Uuid::parse_str(id).is_ok())
}

#[cfg(not(windows))]
fn replace_file(temp_path: &Path, target: &Path) -> std::io::Result<()> {
    fs::rename(temp_path, target)
}

#[cfg(windows)]
fn replace_file(temp_path: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let from = temp_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let to = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let flags = MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH;
    let ok = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), flags) };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn sync_parent_dir(path: &Path) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::File::open(parent)?.sync_all()
}

#[cfg(windows)]
fn sync_parent_dir(_path: &Path) -> std::io::Result<()> {
    // MoveFileExW with MOVEFILE_WRITE_THROUGH waits for the replacement to reach disk.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_existing_content() {
        let directory = std::env::temp_dir().join(format!(
            "simple-table-atomic-write-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&directory).expect("test directory");
        let target = directory.join("document.xlsx");
        fs::write(&target, b"old").expect("old content");

        write_file_atomically(&target, b"new").expect("atomic write");

        assert_eq!(fs::read(&target).expect("saved content"), b"new");
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn temp_write_does_not_replace_or_remove_an_existing_file() {
        let path = std::env::temp_dir().join(format!(
            "simple-table-existing-temp-{}",
            uuid::Uuid::new_v4()
        ));
        fs::write(&path, b"existing").expect("existing file");

        assert!(write_temp_file(&path, b"replacement").is_err());

        assert_eq!(fs::read(&path).expect("preserved file"), b"existing");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn relative_target_synchronizes_the_current_directory() {
        assert!(sync_parent_dir(Path::new("document.xlsx")).is_ok());
    }

    #[test]
    fn replacement_reports_when_content_moved_but_directory_sync_failed() {
        let error = finish_replacement(Err(std::io::Error::other("injected sync failure")))
            .expect_err("replacement should report uncertain durability");

        assert!(matches!(&error, AtomicReplaceError::ReplacedNotDurable(_)));
        assert!(
            error
                .into_app_error()
                .to_string()
                .contains("content was replaced")
        );
    }

    #[test]
    fn orphan_cleanup_removes_owned_and_legacy_temp_files_only() {
        let directory = std::env::temp_dir().join(format!(
            "simple-table-atomic-cleanup-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&directory).expect("test directory");
        let current = temp_path_for_target(&directory.join("document.xlsx"));
        let legacy = directory.join(format!(".document.xlsx.{}.tmp", uuid::Uuid::new_v4()));
        let unrelated = directory.join(".unrelated.tmp");
        fs::write(&current, b"current").expect("current temp");
        fs::write(&legacy, b"legacy").expect("legacy temp");
        fs::write(&unrelated, b"keep").expect("unrelated file");

        cleanup_orphaned_temp_files(&directory).expect("cleanup temp files");

        assert!(!current.exists());
        assert!(!legacy.exists());
        assert!(unrelated.exists());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn orphan_cleanup_runs_before_bounded_storage_scans() {
        use crate::io::marker_store::{MAX_STORAGE_DIRECTORY_ENTRIES, bounded_directory_entries};

        let directory = std::env::temp_dir().join(format!(
            "simple-table-atomic-cleanup-limit-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&directory).expect("test directory");
        for _ in 0..=MAX_STORAGE_DIRECTORY_ENTRIES {
            let temp = temp_path_for_target(&directory.join("document.xlsx"));
            fs::write(temp, []).expect("orphaned temp");
        }
        assert!(bounded_directory_entries(&directory, "test").is_err());

        cleanup_orphaned_temp_files(&directory).expect("cleanup temp files");

        assert!(bounded_directory_entries(&directory, "test").is_ok());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn stale_cleanup_preserves_fresh_and_unrelated_files() {
        let directory = std::env::temp_dir().join(format!(
            "simple-table-atomic-stale-cleanup-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&directory).expect("test directory");
        let owned = temp_path_for_target(&directory.join("document.xlsx"));
        let unrelated = directory.join(".unrelated.tmp");
        fs::write(&owned, b"owned").expect("owned temp");
        fs::write(&unrelated, b"keep").expect("unrelated file");

        cleanup_stale_temp_files(&directory, Duration::from_secs(60 * 60))
            .expect("preserve fresh temp file");
        assert!(owned.exists());

        cleanup_stale_temp_files(&directory, Duration::ZERO).expect("remove stale temp file");
        assert!(!owned.exists());
        assert!(unrelated.exists());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn stale_cleanup_has_a_fixed_directory_scan_budget() {
        let directory = std::env::temp_dir().join(format!(
            "simple-table-atomic-stale-cleanup-limit-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&directory).expect("test directory");
        for _ in 0..=MAX_STALE_TEMP_SCAN_ENTRIES {
            let owned = temp_path_for_target(&directory.join("document.xlsx"));
            fs::write(owned, []).expect("owned temp");
        }

        cleanup_stale_temp_files(&directory, Duration::ZERO).expect("bounded cleanup");

        assert!(
            fs::read_dir(&directory)
                .expect("remaining entries")
                .next()
                .is_some()
        );
        cleanup_orphaned_temp_files(&directory).expect("full startup cleanup");
        assert!(
            fs::read_dir(&directory)
                .expect("empty entries")
                .next()
                .is_none()
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(unix)]
    #[test]
    fn orphan_cleanup_does_not_follow_owned_name_symlinks() {
        use std::os::unix::fs::symlink;

        let directory = std::env::temp_dir().join(format!(
            "simple-table-atomic-cleanup-link-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&directory).expect("test directory");
        let target = directory.join("target");
        let link = temp_path_for_target(&directory.join("document.xlsx"));
        fs::write(&target, b"keep").expect("target");
        symlink(&target, &link).expect("temp symlink");

        cleanup_orphaned_temp_files(&directory).expect("cleanup temp files");

        assert!(target.exists());
        assert!(link.symlink_metadata().is_ok());
        let _ = fs::remove_dir_all(directory);
    }
}
