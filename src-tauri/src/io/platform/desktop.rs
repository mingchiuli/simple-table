use crate::error::AppError;
use crate::io::atomic_file::{
    cleanup_temp_file, replace_temp_file, write_file_atomically, write_temp_file_for_target,
};
use crate::io::document;
use crate::io::file_format::{
    SUPPORTED_SPREADSHEET_EXTENSIONS, file_name_from_path_like, output_name_for_selected_target,
    supported_extension_from_name,
};
use crate::io::projection_limits::{read_input_bytes, validate_input_file_size};
use crate::recent::store::RecentStore;
use crate::types::{PreparedOpenDocument, SavedDocumentResponse};
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::AppHandle;
use tauri_plugin_fs::FilePath;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopOpenFileInfo {
    pub path: String,
    pub file_name: String,
}

static AUTHORIZED_OPEN_PATHS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
static AUTHORIZED_SAVE_PATHS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

pub fn authorize_open_path(path: impl AsRef<Path>) {
    authorize_path(open_paths(), normalize_existing_path(path.as_ref()));
}

pub fn authorize_open_target(target: &str) {
    for candidate in open_target_candidates(target) {
        if is_supported_existing_spreadsheet_path(&candidate) {
            authorize_open_path(candidate);
        }
    }
}

pub fn pick_open_file(app: &AppHandle) -> Result<Option<DesktopOpenFileInfo>, AppError> {
    use tauri_plugin_dialog::DialogExt;

    let Some(path) = app
        .dialog()
        .file()
        .add_filter("Spreadsheet", SUPPORTED_SPREADSHEET_EXTENSIONS)
        .blocking_pick_file()
    else {
        return Ok(None);
    };

    let path = file_path_to_path_buf(path)?;
    authorize_open_path(&path);
    let path = path.to_string_lossy().to_string();
    let file_name = file_name_from_path_like(&path, "unknown");
    Ok(Some(DesktopOpenFileInfo { path, file_name }))
}

pub fn prepare_file(path: &str) -> Result<PreparedOpenDocument, AppError> {
    if !consume_path(open_paths(), &normalize_existing_path(Path::new(path))) {
        return Err(AppError::DocumentStateInvalid(
            "desktop file open path was not selected by the user".to_string(),
        ));
    }
    prepare_file_trusted(path)
}

pub fn prepare_recent_file(app: &AppHandle, id: &str) -> Result<PreparedOpenDocument, AppError> {
    let recent = RecentStore::get_all(app)?
        .into_iter()
        .find(|file| file.id == id)
        .ok_or_else(|| AppError::FileNotFound(id.to_string()))?;
    prepare_file_trusted(&recent.path)
}

fn prepare_file_trusted(path: &str) -> Result<PreparedOpenDocument, AppError> {
    let metadata = fs::metadata(path).map_err(|e| match e.kind() {
        ErrorKind::NotFound => AppError::FileNotFound(path.to_string()),
        _ => AppError::ReadError(e.to_string()),
    })?;
    validate_input_file_size(metadata.len())?;
    let file = fs::File::open(path).map_err(|e| match e.kind() {
        ErrorKind::NotFound => AppError::FileNotFound(path.to_string()),
        _ => AppError::ReadError(e.to_string()),
    })?;
    let bytes = read_input_bytes(file)?;
    document::prepare_open_from_bytes(path.to_string(), bytes, None)
}

pub fn discard_open_file_selection(path: &str) {
    revoke_path(open_paths(), &normalize_existing_path(Path::new(path)));
}

pub fn pick_save_location(app: &AppHandle, default_name: &str) -> Result<Option<String>, AppError> {
    use tauri_plugin_dialog::DialogExt;

    let Some(path) = app
        .dialog()
        .file()
        .add_filter("Spreadsheet", SUPPORTED_SPREADSHEET_EXTENSIONS)
        .set_file_name(default_name)
        .blocking_save_file()
    else {
        return Ok(None);
    };

    let path = file_path_to_path_buf(path)?;
    authorize_path(save_paths(), normalize_target_path(&path));
    Ok(Some(path.to_string_lossy().to_string()))
}

pub fn discard_save_location(path: &str) {
    revoke_path(save_paths(), &normalize_target_path(Path::new(path)));
}

pub fn save_file(
    path: &str,
    document_id: u64,
    base_revision: u64,
) -> Result<SavedDocumentResponse, AppError> {
    ensure_save_path_authorized(path, document_id, base_revision)?;
    let prepared = document::prepare_current_file_save(document_id, base_revision, path)?;
    let target = Path::new(path);
    let temp_path = match write_temp_file_for_target(target, &prepared.bytes) {
        Ok(temp_path) => temp_path,
        Err(error) => {
            document::abort_prepared_file_save(&prepared);
            return Err(error);
        }
    };

    let result = document::commit_current_file_save(path.to_string(), prepared, || {
        replace_temp_file(&temp_path, target)
    });
    if result.is_err() {
        cleanup_temp_file(&temp_path);
    }
    result
}

pub fn export_file(
    app: &AppHandle,
    default_name: &str,
    document_id: u64,
    base_revision: u64,
) -> Result<Option<String>, AppError> {
    use tauri_plugin_dialog::DialogExt;

    let Some(path) = app
        .dialog()
        .file()
        .add_filter("Spreadsheet", SUPPORTED_SPREADSHEET_EXTENSIONS)
        .set_file_name(default_name)
        .blocking_save_file()
    else {
        return Ok(None);
    };

    let path = file_path_to_path_buf(path)?;
    let path_string = path.to_string_lossy().to_string();
    let selected_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| file_name_from_path_like(name, ""));
    let target_path_or_name =
        output_name_for_selected_target(selected_name.as_deref(), default_name);
    let (_, bytes) = document::generate_current_file_bytes_for_target(
        document_id,
        base_revision,
        &target_path_or_name,
    )?;
    write_file_atomically(&path, &bytes)?;
    Ok(Some(path_string))
}

fn file_path_to_path_buf(path: FilePath) -> Result<PathBuf, AppError> {
    match path {
        FilePath::Path(path) => Ok(path),
        FilePath::Url(url) => url
            .to_file_path()
            .map_err(|_| AppError::ReadError("Selected desktop file is not a local path".into())),
    }
}

fn ensure_save_path_authorized(
    path: &str,
    document_id: u64,
    base_revision: u64,
) -> Result<(), AppError> {
    let target = normalize_target_path(Path::new(path));
    if is_current_document_path(&target, document_id, base_revision)? {
        return Ok(());
    }
    if consume_path(save_paths(), &target) {
        return Ok(());
    }
    Err(AppError::DocumentStateInvalid(
        "desktop save target was not selected by the user".to_string(),
    ))
}

fn is_current_document_path(
    target: &Path,
    document_id: u64,
    base_revision: u64,
) -> Result<bool, AppError> {
    let current_path =
        document::inspect_current_file_for_command(document_id, base_revision, |file_data| {
            file_data.path.clone()
        })?;
    if current_path.is_empty() {
        return Ok(false);
    }
    Ok(normalize_target_path(Path::new(&current_path)) == target)
}

fn open_paths() -> &'static Mutex<HashSet<PathBuf>> {
    AUTHORIZED_OPEN_PATHS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn save_paths() -> &'static Mutex<HashSet<PathBuf>> {
    AUTHORIZED_SAVE_PATHS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn authorize_path(paths: &'static Mutex<HashSet<PathBuf>>, path: PathBuf) {
    if let Ok(mut paths) = paths.lock() {
        paths.insert(path);
    }
}

fn consume_path(paths: &'static Mutex<HashSet<PathBuf>>, path: &Path) -> bool {
    paths
        .lock()
        .map(|mut paths| paths.remove(path))
        .unwrap_or(false)
}

fn revoke_path(paths: &'static Mutex<HashSet<PathBuf>>, path: &Path) {
    if let Ok(mut paths) = paths.lock() {
        paths.remove(path);
    }
}

fn normalize_existing_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn normalize_target_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = fs::canonicalize(path) {
        return canonical;
    }
    let Some(file_name) = path.file_name() else {
        return path.to_path_buf();
    };
    path.parent()
        .and_then(|parent| fs::canonicalize(parent).ok())
        .map(|parent| parent.join(file_name))
        .unwrap_or_else(|| path.to_path_buf())
}

fn open_target_candidates(target: &str) -> Vec<PathBuf> {
    let mut candidates = vec![PathBuf::from(target)];
    if let Some(file_uri_path) = file_uri_to_path(target) {
        candidates.push(file_uri_path);
    }
    candidates
}

fn file_uri_to_path(target: &str) -> Option<PathBuf> {
    let rest = target.strip_prefix("file://")?;
    let decoded = percent_decode(rest);
    #[cfg(windows)]
    {
        let path = decoded.strip_prefix('/').unwrap_or(&decoded);
        Some(PathBuf::from(path))
    }
    #[cfg(not(windows))]
    {
        Some(PathBuf::from(decoded))
    }
}

fn is_supported_existing_spreadsheet_path(path: &Path) -> bool {
    path.is_file() && supported_extension_from_name(&path.to_string_lossy()).is_some()
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            decoded.push((high << 4) | low);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }

    String::from_utf8(decoded).unwrap_or_else(|_| value.to_string())
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("simple-table-desktop-platform-{name}"))
    }

    #[test]
    fn launch_authorization_only_accepts_existing_supported_spreadsheets() {
        let supported = temp_path("supported.xlsx");
        let unsupported = temp_path("unsupported.txt");
        std::fs::write(&supported, b"").expect("write supported");
        std::fs::write(&unsupported, b"").expect("write unsupported");

        assert!(is_supported_existing_spreadsheet_path(&supported));
        assert!(!is_supported_existing_spreadsheet_path(&unsupported));
        assert!(!is_supported_existing_spreadsheet_path(&temp_path(
            "missing.xlsx"
        )));

        let _ = std::fs::remove_file(supported);
        let _ = std::fs::remove_file(unsupported);
    }

    #[test]
    fn file_uri_launch_targets_are_decoded_to_local_paths() {
        let path = file_uri_to_path("file:///tmp/simple%20table.xlsx").expect("file uri");

        assert_eq!(path, PathBuf::from("/tmp/simple table.xlsx"));
    }

    #[test]
    fn file_association_url_authorizes_the_resolved_spreadsheet_path() {
        let path = temp_path("file-association.xlsx");
        std::fs::write(&path, b"").expect("write associated file");
        let target = format!("file://{}", path.to_string_lossy());

        authorize_open_target(&target);

        assert!(consume_path(open_paths(), &normalize_existing_path(&path)));
        let _ = std::fs::remove_file(path);
    }
}
