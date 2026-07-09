use crate::error::AppError;
use crate::io::file_format::default_spreadsheet_file_name;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn write_file_atomically(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let temp_path = write_temp_file_for_target(path, bytes)?;
    let result = replace_temp_file(&temp_path, path);
    if result.is_err() {
        cleanup_temp_file(&temp_path);
    }
    result
}

pub fn write_temp_file_for_target(target: &Path, bytes: &[u8]) -> Result<PathBuf, AppError> {
    let temp_path = temporary_path_for(target);
    write_temp_file(&temp_path, bytes)?;
    Ok(temp_path)
}

pub fn replace_temp_file(temp_path: &Path, target: &Path) -> Result<(), AppError> {
    replace_file(temp_path, target).map_err(|error| AppError::WriteError(error.to_string()))?;
    sync_parent_dir(target);
    Ok(())
}

pub fn cleanup_temp_file(temp_path: &Path) {
    let _ = fs::remove_file(temp_path);
}

fn temporary_path_for(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| default_spreadsheet_file_name("simple-table"));
    parent.join(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()))
}

fn write_temp_file(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let mut file = fs::File::create(path).map_err(|e| AppError::WriteError(e.to_string()))?;
    file.write_all(bytes)
        .map_err(|e| AppError::WriteError(e.to_string()))?;
    file.sync_all()
        .map_err(|e| AppError::WriteError(e.to_string()))
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

fn sync_parent_dir(path: &Path) {
    let Some(parent) = path.parent() else {
        return;
    };
    if let Ok(dir) = fs::File::open(parent) {
        let _ = dir.sync_all();
    }
}
