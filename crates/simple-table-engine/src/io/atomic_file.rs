use crate::error::AppError;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const TEMP_FILE_PREFIX: &str = ".simple-table-atomic-";
const TEMP_FILE_SUFFIX: &str = ".tmp";

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
}
