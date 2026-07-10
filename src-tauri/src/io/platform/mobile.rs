#![cfg_attr(test, allow(dead_code))]

use crate::error::AppError;
use crate::io::atomic_file::{cleanup_temp_file, replace_temp_file, write_temp_file_for_target};
use crate::io::document;
use crate::io::file_format::{
    SUPPORTED_SPREADSHEET_EXTENSIONS, file_name_from_path_like, output_name_for_selected_target,
    supported_extension_or_default,
};
use crate::io::projection_limits::{read_input_bytes, validate_input_file_size};
use crate::io::transient_files::transient_file_registry;
use crate::types::{PreparedOpenDocument, SavedDocumentResponse};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};
use tauri_plugin_fs::FilePath;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedFileInfo {
    pub path: String,
    pub original_path: String,
    pub file_name: String,
}

pub(super) fn extension_from_name(file_name: &str) -> String {
    supported_extension_or_default(file_name)
}

pub(super) fn mobile_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| AppError::ReadError(format!("Failed to get app local data dir: {}", e)))?
        .join("files");
    fs::create_dir_all(&dir)
        .map_err(|e| AppError::WriteError(format!("Failed to create app file dir: {}", e)))?;
    Ok(dir)
}

pub(super) fn unique_import_path(app: &AppHandle, file_name: &str) -> Result<PathBuf, AppError> {
    Ok(mobile_dir(app)?.join(format!(
        "{}.{}",
        uuid::Uuid::new_v4(),
        extension_from_name(file_name)
    )))
}

pub(super) fn register_transient_path(app: &AppHandle, path: &Path) -> Result<(), AppError> {
    let target = validated_mobile_files_path(app, path)?;
    register_transient_target(target)
}

pub(super) fn register_created_transient_path(
    app: &AppHandle,
    path: &Path,
) -> Result<(), AppError> {
    if let Err(error) = register_transient_path(app, path) {
        let _ = fs::remove_file(path);
        return Err(error);
    }
    Ok(())
}

pub(super) fn write_with_official_fs(
    app: &AppHandle,
    path: FilePath,
    bytes: &[u8],
) -> Result<(), AppError> {
    use tauri_plugin_fs::{FsExt, OpenOptions};

    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    let mut file = app
        .fs()
        .open(path, options)
        .map_err(|e| AppError::WriteError(format!("Failed to open file for writing: {}", e)))?;
    file.write_all(bytes)
        .map_err(|e| AppError::WriteError(format!("Failed to write file: {}", e)))
}

pub(super) fn read_with_official_fs(app: &AppHandle, path: FilePath) -> Result<Vec<u8>, AppError> {
    use tauri_plugin_fs::{FsExt, OpenOptions};

    let mut options = OpenOptions::new();
    options.read(true);
    let file = app
        .fs()
        .open(path, options)
        .map_err(|e| AppError::ReadError(format!("Failed to open selected file: {e}")))?;
    read_input_bytes(file)
}

pub(super) fn write_path_with_official_fs(
    app: &AppHandle,
    path: PathBuf,
    bytes: &[u8],
) -> Result<(), AppError> {
    write_with_official_fs(app, FilePath::from(path), bytes)
}

fn selected_file_name(path: &FilePath) -> Option<String> {
    match path {
        FilePath::Path(path) => path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| file_name_from_path_like(name, "")),
        FilePath::Url(url) => url
            .to_file_path()
            .ok()
            .and_then(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| file_name_from_path_like(name, ""))
            })
            .or_else(|| {
                url.path_segments()
                    .and_then(|mut segments| segments.next_back())
                    .filter(|segment| !segment.is_empty())
                    .map(|segment| file_name_from_path_like(segment, ""))
            }),
    }
    .filter(|name| !name.is_empty())
}

pub fn prepare_file(app: &AppHandle, path: &str) -> Result<PreparedOpenDocument, AppError> {
    use tauri_plugin_fs::FsExt;

    let target = validated_mobile_files_path(app, Path::new(path))?;
    if !target.exists() {
        return Err(AppError::FileNotFound(path.to_string()));
    }
    let metadata = fs::metadata(&target)
        .map_err(|e| AppError::ReadError(format!("Failed to inspect file: {}", e)))?;
    validate_input_file_size(metadata.len())?;

    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string();
    let bytes = app
        .fs()
        .read(FilePath::from(target.clone()))
        .map_err(|e| AppError::ReadError(format!("Failed to read file: {}", e)))?;

    document::prepare_open_from_bytes(target.to_string_lossy().to_string(), bytes, Some(file_name))
}

pub fn discard_transient_file(app: &AppHandle, path: &str) -> Result<(), AppError> {
    let target = take_registered_transient_path(app, Path::new(path))?;
    match fs::remove_file(&target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => {
            let message = format!("Failed to remove unused transient file: {}", error);
            let _ = register_transient_target(target);
            Err(AppError::WriteError(message))
        }
    }
}

fn validated_mobile_files_path(app: &AppHandle, path: &Path) -> Result<PathBuf, AppError> {
    let files_dir = mobile_dir(app)?.canonicalize().map_err(|e| {
        AppError::DocumentStateInvalid(format!("Failed to resolve app file dir: {}", e))
    })?;
    validated_mobile_files_path_in_dir(&files_dir, path)
}

fn validated_mobile_files_path_in_dir(files_dir: &Path, path: &Path) -> Result<PathBuf, AppError> {
    let file_name = path.file_name().ok_or_else(|| {
        AppError::DocumentStateInvalid("Mobile file path has no file name".to_string())
    })?;
    let target_parent = path.parent().ok_or_else(|| {
        AppError::DocumentStateInvalid("Mobile file path has no parent directory".to_string())
    })?;
    let target_parent = target_parent.canonicalize().map_err(|e| {
        AppError::DocumentStateInvalid(format!("Failed to resolve transient file dir: {}", e))
    })?;

    if target_parent != files_dir {
        return Err(AppError::DocumentStateInvalid(
            "Refusing to use a file outside the mobile files directory".to_string(),
        ));
    }

    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(AppError::DocumentStateInvalid(
            "Refusing to use a symbolic link in the mobile files directory".to_string(),
        )),
        Ok(metadata) if metadata.is_dir() => Err(AppError::DocumentStateInvalid(
            "Mobile file path must reference a file".to_string(),
        )),
        Ok(_) => {
            let canonical_target = path.canonicalize().map_err(|e| {
                AppError::DocumentStateInvalid(format!("Failed to resolve mobile file: {}", e))
            })?;
            if canonical_target.parent() != Some(files_dir) {
                return Err(AppError::DocumentStateInvalid(
                    "Refusing to use a file outside the mobile files directory".to_string(),
                ));
            }
            Ok(canonical_target)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(files_dir.join(file_name)),
        Err(error) => Err(AppError::DocumentStateInvalid(format!(
            "Failed to inspect mobile file: {}",
            error
        ))),
    }
}

fn register_transient_target(target: PathBuf) -> Result<(), AppError> {
    transient_file_registry().register(target)
}

fn take_registered_transient_path(app: &AppHandle, path: &Path) -> Result<PathBuf, AppError> {
    let target = validated_mobile_files_path(app, path)?;
    transient_file_registry().take(&target)
}

fn adopt_transient_path_if_registered(app: &AppHandle, path: &Path) {
    let Ok(target) = validated_mobile_files_path(app, path) else {
        return;
    };
    let _ = transient_file_registry().adopt_if_registered(&target);
}

pub fn save_file(
    app: &AppHandle,
    path: &str,
    document_id: u64,
    base_revision: u64,
) -> Result<SavedDocumentResponse, AppError> {
    let target = validated_mobile_files_path(app, Path::new(path))?;
    let target_path = target.to_string_lossy().to_string();
    let prepared = document::prepare_current_file_save(document_id, base_revision, &target_path)?;
    let temp_path = match write_temp_file_for_target(&target, &prepared.bytes) {
        Ok(temp_path) => temp_path,
        Err(error) => {
            document::abort_prepared_file_save(&prepared);
            return Err(error);
        }
    };

    let result = document::commit_current_file_save(target_path, prepared, || {
        replace_temp_file(&temp_path, &target)
    });
    if result.is_err() {
        cleanup_temp_file(&temp_path);
    } else {
        adopt_transient_path_if_registered(app, &target);
    }
    result
}

pub fn reserve_save_location(app: &AppHandle, file_name: &str) -> Result<String, AppError> {
    let path = mobile_dir(app)?.join(format!(
        "{}.{}",
        uuid::Uuid::new_v4(),
        extension_from_name(file_name)
    ));
    write_path_with_official_fs(app, path.clone(), &[])?;
    register_created_transient_path(app, &path)?;

    Ok(path.to_string_lossy().to_string())
}

pub fn export_file(
    app: &AppHandle,
    default_name: &str,
    document_id: u64,
    base_revision: u64,
) -> Result<Option<String>, AppError> {
    use tauri_plugin_dialog::{DialogExt, PickerMode};

    let dest = match app
        .dialog()
        .file()
        .add_filter("Spreadsheet", SUPPORTED_SPREADSHEET_EXTENSIONS)
        .set_picker_mode(PickerMode::Document)
        .set_file_name(default_name)
        .blocking_save_file()
    {
        Some(path) => path,
        None => return Ok(None),
    };

    let selected_name = selected_file_name(&dest);
    let target_path_or_name =
        output_name_for_selected_target(selected_name.as_deref(), default_name);
    let (_, bytes) = document::generate_current_file_bytes_for_target(
        document_id,
        base_revision,
        &target_path_or_name,
    )?;

    write_with_official_fs(app, dest.clone(), &bytes)
        .map_err(|e| AppError::WriteError(format!("Failed to export file: {}", e)))?;

    Ok(Some(dest.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{extension_from_name, validated_mobile_files_path_in_dir};
    use crate::error::AppError;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn extension_from_name_uses_supported_extension_or_xlsx_default() {
        assert_eq!(extension_from_name("book.xlsx"), "xlsx");
        assert_eq!(extension_from_name("data.CSV"), "csv");
        assert_eq!(extension_from_name("untitled"), "xlsx");
        assert_eq!(extension_from_name("unsupported.bin"), "xlsx");
    }

    #[test]
    fn mobile_file_path_accepts_only_direct_files_dir_children() {
        let test_dir = TestDir::new("direct-child");
        let files_dir = test_dir.path.join("files");
        fs::create_dir_all(&files_dir).expect("files dir");
        let file_path = files_dir.join("book.xlsx");
        fs::write(&file_path, b"test").expect("file");

        let files_dir = files_dir.canonicalize().expect("canonical files dir");

        assert_eq!(
            validated_mobile_files_path_in_dir(&files_dir, &file_path).expect("validated path"),
            file_path.canonicalize().expect("canonical file")
        );
    }

    #[test]
    fn mobile_file_path_allows_missing_direct_child_for_reserved_save() {
        let test_dir = TestDir::new("missing-child");
        let files_dir = test_dir.path.join("files");
        fs::create_dir_all(&files_dir).expect("files dir");
        let files_dir = files_dir.canonicalize().expect("canonical files dir");
        let target = files_dir.join("reserved.xlsx");

        assert_eq!(
            validated_mobile_files_path_in_dir(&files_dir, &target).expect("validated path"),
            target
        );
    }

    #[test]
    fn mobile_file_path_rejects_paths_outside_files_dir() {
        let test_dir = TestDir::new("outside");
        let files_dir = test_dir.path.join("files");
        let outside_dir = test_dir.path.join("outside");
        fs::create_dir_all(&files_dir).expect("files dir");
        fs::create_dir_all(&outside_dir).expect("outside dir");
        let outside_file = outside_dir.join("book.xlsx");
        fs::write(&outside_file, b"test").expect("outside file");
        let files_dir = files_dir.canonicalize().expect("canonical files dir");

        assert_document_state_invalid(validated_mobile_files_path_in_dir(
            &files_dir,
            &outside_file,
        ));
    }

    #[test]
    fn mobile_file_path_rejects_nested_files_dir_children() {
        let test_dir = TestDir::new("nested");
        let files_dir = test_dir.path.join("files");
        let nested_dir = files_dir.join("nested");
        fs::create_dir_all(&nested_dir).expect("nested dir");
        let nested_file = nested_dir.join("book.xlsx");
        fs::write(&nested_file, b"test").expect("nested file");
        let files_dir = files_dir.canonicalize().expect("canonical files dir");

        assert_document_state_invalid(validated_mobile_files_path_in_dir(&files_dir, &nested_file));
    }

    #[cfg(unix)]
    #[test]
    fn mobile_file_path_rejects_symbolic_links() {
        use std::os::unix::fs::symlink;

        let test_dir = TestDir::new("symlink");
        let files_dir = test_dir.path.join("files");
        let outside_dir = test_dir.path.join("outside");
        fs::create_dir_all(&files_dir).expect("files dir");
        fs::create_dir_all(&outside_dir).expect("outside dir");
        let outside_file = outside_dir.join("book.xlsx");
        fs::write(&outside_file, b"test").expect("outside file");
        let link_path = files_dir.join("link.xlsx");
        symlink(&outside_file, &link_path).expect("symlink");
        let files_dir = files_dir.canonicalize().expect("canonical files dir");

        assert_document_state_invalid(validated_mobile_files_path_in_dir(&files_dir, &link_path));
    }

    fn assert_document_state_invalid(result: Result<PathBuf, AppError>) {
        assert!(matches!(result, Err(AppError::DocumentStateInvalid(_))));
    }

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "simple-table-mobile-paths-{}-{}",
                label,
                uuid::Uuid::new_v4()
            ));
            fs::create_dir_all(&path).expect("test dir");
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
