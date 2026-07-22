#![cfg_attr(test, allow(dead_code))]

use crate::document_format::{
    SUPPORTED_SPREADSHEET_EXTENSIONS, file_name_from_path_like, output_name_for_selected_target,
    supported_extension_or_default,
};
use crate::error::AppError;
use crate::io::input_limits::{read_input_bytes, validate_input_file_size};
use crate::io::open_file_input::OpenFileInput;
use crate::io::transient_files::{
    TransientFilePurpose, clear_persistent_marker, completed_persisted_save_locations,
    reconcile_persisted_transient_files, write_persistent_marker,
};
use crate::io::{
    managed_documents,
    managed_documents::{ManagedDocumentCatalog, ManagedDocumentRecord},
    transient_files::TransientFileRegistry,
};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tauri::{AppHandle, Manager};
use tauri_plugin_fs::FilePath;

#[derive(Clone)]
pub struct MobileFileRuntime {
    storage_directory: Arc<OnceLock<Result<PathBuf, AppError>>>,
    transient_files: Arc<TransientFileRegistry>,
    managed_documents: ManagedDocumentCatalog,
}

impl Default for MobileFileRuntime {
    fn default() -> Self {
        Self {
            storage_directory: Arc::new(OnceLock::new()),
            transient_files: Arc::new(TransientFileRegistry::default()),
            managed_documents: ManagedDocumentCatalog::default(),
        }
    }
}

impl MobileFileRuntime {
    pub(crate) fn transient_files(&self) -> &TransientFileRegistry {
        &self.transient_files
    }

    pub(crate) fn managed_documents(&self) -> &ManagedDocumentCatalog {
        &self.managed_documents
    }

    pub(crate) fn begin_transient_document_adoption(
        &self,
        target: &Path,
        file_name: &str,
    ) -> Result<managed_documents::ManagedDocumentAdoption, AppError> {
        managed_documents::begin_transient_document_adoption(
            &self.managed_documents,
            Arc::clone(&self.transient_files),
            target,
            file_name,
        )
    }

    #[cfg(test)]
    pub(crate) fn is_isolated_from(&self, other: &Self) -> bool {
        !Arc::ptr_eq(&self.storage_directory, &other.storage_directory)
            && !Arc::ptr_eq(&self.transient_files, &other.transient_files)
            && !self
                .managed_documents
                .is_same_instance(&other.managed_documents)
    }
}

pub(super) fn extension_from_name(file_name: &str) -> String {
    supported_extension_or_default(file_name)
}

fn resolve_mobile_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| AppError::ReadError(format!("Failed to get app local data dir: {}", e)))?
        .join("files");
    fs::create_dir_all(&dir)
        .map_err(|e| AppError::WriteError(format!("Failed to create app file dir: {}", e)))?;
    Ok(dir)
}

fn initialize_mobile_storage(
    runtime: &MobileFileRuntime,
    app: &AppHandle,
) -> Result<PathBuf, AppError> {
    let dir = resolve_mobile_dir(app)?;
    for managed in managed_documents::managed_documents(runtime.managed_documents(), &dir)? {
        clear_persistent_marker(&managed.path);
    }
    for target in completed_persisted_save_locations(&dir)? {
        if let Err(error) =
            managed_documents::recover_completed_save(runtime.managed_documents(), &target)
        {
            eprintln!(
                "Failed to recover completed mobile save {}: {error}",
                target.display()
            );
        }
    }
    reconcile_persisted_transient_files(&dir)?;
    Ok(dir)
}

pub(super) fn mobile_dir(
    runtime: &MobileFileRuntime,
    app: &AppHandle,
) -> Result<PathBuf, AppError> {
    cached_mobile_storage_directory(&runtime.storage_directory, || {
        initialize_mobile_storage(runtime, app)
    })
}

fn cached_mobile_storage_directory(
    cache: &OnceLock<Result<PathBuf, AppError>>,
    initialize: impl FnOnce() -> Result<PathBuf, AppError>,
) -> Result<PathBuf, AppError> {
    cache.get_or_init(initialize).clone()
}

pub(super) fn unique_import_path(
    runtime: &MobileFileRuntime,
    app: &AppHandle,
    file_name: &str,
) -> Result<PathBuf, AppError> {
    Ok(mobile_dir(runtime, app)?.join(format!(
        "{}.{}",
        uuid::Uuid::new_v4(),
        extension_from_name(file_name)
    )))
}

pub(super) fn register_transient_path(
    runtime: &MobileFileRuntime,
    app: &AppHandle,
    path: &Path,
    purpose: TransientFilePurpose,
) -> Result<(), AppError> {
    let target = validated_mobile_files_path(runtime, app, path)?;
    register_transient_target(runtime, target, purpose)
}

pub(super) fn register_created_transient_path(
    runtime: &MobileFileRuntime,
    app: &AppHandle,
    path: &Path,
    purpose: TransientFilePurpose,
) -> Result<(), AppError> {
    if let Err(error) = register_transient_path(runtime, app, path, purpose) {
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

pub fn read_open_file(
    runtime: &MobileFileRuntime,
    app: &AppHandle,
    path: &str,
) -> Result<OpenFileInput, AppError> {
    use tauri_plugin_fs::{FsExt, OpenOptions};

    let target = validated_mobile_files_path(runtime, app, Path::new(path))?;
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
    let mut options = OpenOptions::new();
    options.read(true);
    let file = app
        .fs()
        .open(FilePath::from(target.clone()), options)
        .map_err(|e| AppError::ReadError(format!("Failed to open file: {}", e)))?;
    Ok(OpenFileInput {
        path: target.to_string_lossy().to_string(),
        bytes: read_input_bytes(file)?,
        file_name: Some(file_name),
    })
}

pub fn discard_transient_file(
    runtime: &MobileFileRuntime,
    app: &AppHandle,
    path: &str,
    purpose: TransientFilePurpose,
) -> Result<(), AppError> {
    let target = take_registered_transient_path(runtime, app, Path::new(path), purpose)?;
    match fs::remove_file(&target) {
        Ok(()) => {
            clear_persistent_marker(&target);
            Ok(())
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            clear_persistent_marker(&target);
            Ok(())
        }
        Err(error) => {
            let message = format!("Failed to remove unused transient file: {}", error);
            let _ = register_transient_target(runtime, target, purpose);
            Err(AppError::WriteError(message))
        }
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub(crate) fn remove_managed_file_if_inactive(
    runtime: &MobileFileRuntime,
    app: &AppHandle,
    path: &str,
    active_document_path: Option<&str>,
) -> Result<bool, AppError> {
    let target = validated_mobile_files_path(runtime, app, Path::new(path))?;
    if active_document_path.is_some_and(|active| Path::new(active) == target)
        || runtime.transient_files().contains(&target)?
    {
        return Ok(false);
    }

    match fs::remove_file(&target) {
        Ok(()) => {
            managed_documents::clear_managed_document(runtime.managed_documents(), &target)?;
            Ok(true)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            managed_documents::clear_managed_document(runtime.managed_documents(), &target)?;
            Ok(true)
        }
        Err(error) => Err(AppError::WriteError(format!(
            "Failed to remove managed mobile document: {error}"
        ))),
    }
}

pub(crate) fn validated_mobile_files_path(
    runtime: &MobileFileRuntime,
    app: &AppHandle,
    path: &Path,
) -> Result<PathBuf, AppError> {
    let files_dir = mobile_dir(runtime, app)?.canonicalize().map_err(|e| {
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

fn register_transient_target(
    runtime: &MobileFileRuntime,
    target: PathBuf,
    purpose: TransientFilePurpose,
) -> Result<(), AppError> {
    runtime
        .transient_files()
        .register(target.clone(), purpose)?;
    if let Err(error) = write_persistent_marker(&target, purpose) {
        let _ = runtime.transient_files().adopt_if_registered(&target);
        return Err(error);
    }
    Ok(())
}

fn take_registered_transient_path(
    runtime: &MobileFileRuntime,
    app: &AppHandle,
    path: &Path,
    purpose: TransientFilePurpose,
) -> Result<PathBuf, AppError> {
    let target = validated_mobile_files_path(runtime, app, path)?;
    runtime.transient_files().take(&target, purpose)
}

pub(crate) fn reconcile_transient_files(
    runtime: &MobileFileRuntime,
    app: &AppHandle,
) -> Result<(), AppError> {
    mobile_dir(runtime, app).map(|_| ())
}

pub(crate) fn managed_document_records(
    runtime: &MobileFileRuntime,
    app: &AppHandle,
) -> Result<Vec<ManagedDocumentRecord>, AppError> {
    managed_documents::managed_documents(runtime.managed_documents(), &mobile_dir(runtime, app)?)
}

pub(crate) fn migrate_managed_document(
    runtime: &MobileFileRuntime,
    app: &AppHandle,
    path: &str,
    file_name: &str,
    id: &str,
    adopted_at_millis: i64,
) -> Result<(), AppError> {
    let target = validated_mobile_files_path(runtime, app, Path::new(path))?;
    managed_documents::migrate_existing_document(
        runtime.managed_documents(),
        &target,
        file_name,
        id,
        adopted_at_millis,
    )
}

pub(crate) fn ensure_save_target_authorized(
    runtime: &MobileFileRuntime,
    target: &Path,
    current_document_path: &str,
) -> Result<(), AppError> {
    let is_reserved = runtime
        .transient_files()
        .contains_for(target, TransientFilePurpose::SaveLocation)?;
    if is_save_target_authorized(current_document_path, target, is_reserved) {
        return Ok(());
    }
    Err(AppError::DocumentStateInvalid(
        "mobile save target is neither the current document nor a reserved save location"
            .to_string(),
    ))
}

fn is_save_target_authorized(current_path: &str, target: &Path, is_reserved: bool) -> bool {
    is_reserved || (!current_path.is_empty() && Path::new(current_path) == target)
}

pub fn reserve_save_location(
    runtime: &MobileFileRuntime,
    app: &AppHandle,
    file_name: &str,
) -> Result<String, AppError> {
    let path = mobile_dir(runtime, app)?.join(format!(
        "{}.{}",
        uuid::Uuid::new_v4(),
        extension_from_name(file_name)
    ));
    write_path_with_official_fs(app, path.clone(), &[])?;
    register_created_transient_path(runtime, app, &path, TransientFilePurpose::SaveLocation)?;

    Ok(path.to_string_lossy().to_string())
}

pub(crate) struct MobileExportTarget {
    pub destination: FilePath,
    pub destination_string: String,
    pub target_path_or_name: String,
}

pub(crate) fn pick_export_target(
    app: &AppHandle,
    default_name: &str,
) -> Result<Option<MobileExportTarget>, AppError> {
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

    Ok(Some(MobileExportTarget {
        destination_string: dest.to_string(),
        target_path_or_name: output_name_for_selected_target(
            selected_file_name(&dest).as_deref(),
            default_name,
        ),
        destination: dest,
    }))
}

pub(crate) fn write_export_target(
    app: &AppHandle,
    target: &MobileExportTarget,
    bytes: &[u8],
) -> Result<(), AppError> {
    write_with_official_fs(app, target.destination.clone(), bytes)
}

#[cfg(test)]
mod tests {
    use super::{
        cached_mobile_storage_directory, extension_from_name, is_save_target_authorized,
        validated_mobile_files_path_in_dir,
    };
    use crate::error::AppError;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, OnceLock};
    use std::thread;

    #[test]
    fn extension_from_name_uses_supported_extension_or_xlsx_default() {
        assert_eq!(extension_from_name("book.xlsx"), "xlsx");
        assert_eq!(extension_from_name("data.CSV"), "csv");
        assert_eq!(extension_from_name("untitled"), "xlsx");
        assert_eq!(extension_from_name("unsupported.bin"), "xlsx");
    }

    #[test]
    fn save_target_authorization_accepts_only_current_or_reserved_paths() {
        let current = PathBuf::from("/files/current.xlsx");
        let reserved = PathBuf::from("/files/reserved.xlsx");
        let unrelated = PathBuf::from("/files/unrelated.xlsx");

        assert!(is_save_target_authorized(
            current.to_str().unwrap(),
            &current,
            false
        ));
        assert!(is_save_target_authorized("", &reserved, true));
        assert!(!is_save_target_authorized(
            current.to_str().unwrap(),
            &unrelated,
            false
        ));
    }

    #[test]
    fn mobile_storage_initialization_is_shared_by_concurrent_callers() {
        let cache = Arc::new(OnceLock::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let mut workers = Vec::new();
        for _ in 0..16 {
            let cache = Arc::clone(&cache);
            let calls = Arc::clone(&calls);
            workers.push(thread::spawn(move || {
                cached_mobile_storage_directory(&cache, || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(PathBuf::from("/mobile/files"))
                })
            }));
        }

        for worker in workers {
            assert_eq!(
                worker.join().expect("worker").unwrap(),
                PathBuf::from("/mobile/files")
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn mobile_storage_initialization_failure_is_stable() {
        let cache = OnceLock::new();
        let calls = AtomicUsize::new(0);
        for _ in 0..2 {
            assert!(matches!(
                cached_mobile_storage_directory(&cache, || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Err(AppError::ReadError("failed initialization".to_string()))
                }),
                Err(AppError::ReadError(_))
            ));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
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
