use crate::document_format::{
    SUPPORTED_SPREADSHEET_EXTENSIONS, file_name_from_path_like, output_name_for_selected_target,
    supported_extension_from_name,
};
use crate::error::AppError;
use crate::io::atomic_file::write_file_atomically;
use crate::io::input_limits::{read_input_bytes, validate_input_file_size};
use crate::io::open_file_input::{OpenFileInput, OpenFileSelection};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tauri_plugin_fs::FilePath;

const MAX_AUTHORIZED_PATHS: usize = 64;
const PATH_AUTHORIZATION_TTL: Duration = Duration::from_secs(30 * 60);
const OPEN_TARGET_CLAIM_TTL: Duration = Duration::from_secs(60);

#[derive(Default)]
struct DesktopFileRuntimeInner {
    open_paths: Mutex<PathAuthorizationRegistry>,
    save_paths: Mutex<PathAuthorizationRegistry>,
    open_targets: Mutex<OpenTargetQueue>,
}

#[derive(Clone, Default)]
pub struct DesktopFileRuntime {
    inner: Arc<DesktopFileRuntimeInner>,
}

#[derive(Default)]
struct PathAuthorizationRegistry {
    entries: HashMap<PathBuf, Instant>,
    order: VecDeque<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenTargetClaim {
    pub claim_id: String,
    pub path: String,
}

#[derive(Default)]
struct OpenTargetQueue {
    pending: VecDeque<String>,
    claimed: HashMap<String, ClaimedOpenTarget>,
}

struct ClaimedOpenTarget {
    path: String,
    claimed_at: Instant,
}

impl OpenTargetQueue {
    fn enqueue(&mut self, path: String) -> Vec<String> {
        self.enqueue_at(path, Instant::now())
    }

    fn enqueue_at(&mut self, path: String, now: Instant) -> Vec<String> {
        self.requeue_expired(now);
        if self.claimed.values().any(|claim| claim.path == path) {
            return Vec::new();
        }
        self.pending.retain(|entry| entry != &path);
        self.pending.push_back(path);
        let mut evicted = Vec::new();
        while self.pending.len().saturating_add(self.claimed.len()) > MAX_AUTHORIZED_PATHS {
            let Some(path) = self.pending.pop_front() else {
                break;
            };
            evicted.push(path);
        }
        evicted
    }

    fn claim(&mut self) -> Option<OpenTargetClaim> {
        self.claim_at(Instant::now())
    }

    fn claim_at(&mut self, now: Instant) -> Option<OpenTargetClaim> {
        self.requeue_expired(now);
        let path = self.pending.pop_front()?;
        let claim_id = uuid::Uuid::new_v4().to_string();
        self.claimed.insert(
            claim_id.clone(),
            ClaimedOpenTarget {
                path: path.clone(),
                claimed_at: now,
            },
        );
        Some(OpenTargetClaim { claim_id, path })
    }

    fn acknowledge(&mut self, claim_id: &str) -> (Option<String>, bool) {
        let path = self.claimed.remove(claim_id).map(|claim| claim.path);
        self.requeue_expired(Instant::now());
        (path, !self.pending.is_empty())
    }

    fn release(&mut self, claim_id: &str) -> Option<String> {
        let claim = self.claimed.remove(claim_id)?;
        if !self.pending.contains(&claim.path) {
            self.pending.push_front(claim.path.clone());
        }
        Some(claim.path)
    }

    fn requeue_expired(&mut self, now: Instant) {
        let mut expired = self
            .claimed
            .iter()
            .filter(|(_, claim)| {
                now.saturating_duration_since(claim.claimed_at) >= OPEN_TARGET_CLAIM_TTL
            })
            .map(|(claim_id, claim)| (claim.claimed_at, claim_id.clone()))
            .collect::<Vec<_>>();
        expired.sort_by_key(|(claimed_at, _)| *claimed_at);
        for (_, claim_id) in expired.into_iter().rev() {
            self.release(&claim_id);
        }
    }
}

impl PathAuthorizationRegistry {
    fn authorize(&mut self, path: PathBuf) {
        self.authorize_at(path, Instant::now());
    }

    fn authorize_at(&mut self, path: PathBuf, now: Instant) {
        self.prune_expired(now);
        if self.entries.insert(path.clone(), now).is_some() {
            self.order.retain(|entry| entry != &path);
        }
        self.order.push_back(path);
        while self.entries.len() > MAX_AUTHORIZED_PATHS {
            let Some(expired) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&expired);
        }
    }

    fn consume(&mut self, path: &Path) -> bool {
        self.consume_at(path, Instant::now())
    }

    fn consume_at(&mut self, path: &Path, now: Instant) -> bool {
        self.prune_expired(now);
        let removed = self.entries.remove(path).is_some();
        if removed {
            self.order.retain(|entry| entry != path);
        }
        removed
    }

    fn revoke(&mut self, path: &Path) {
        self.entries.remove(path);
        self.order.retain(|entry| entry != path);
    }

    fn prune_expired(&mut self, now: Instant) {
        self.entries.retain(|_, authorized_at| {
            now.saturating_duration_since(*authorized_at) < PATH_AUTHORIZATION_TTL
        });
        self.order.retain(|path| self.entries.contains_key(path));
    }
}

pub fn authorize_open_path(runtime: &DesktopFileRuntime, path: impl AsRef<Path>) {
    authorize_path(
        &runtime.inner.open_paths,
        normalize_existing_path(path.as_ref()),
    );
}

pub fn enqueue_open_target(runtime: &DesktopFileRuntime, target: &str) -> bool {
    let Some(path) = resolve_open_target(target) else {
        return false;
    };
    let path = path.to_string_lossy().to_string();
    let Ok(mut queue) = runtime.inner.open_targets.lock() else {
        return false;
    };
    let evicted = queue.enqueue(path.clone());
    let accepted =
        queue.pending.contains(&path) || queue.claimed.values().any(|claim| claim.path == path);
    if accepted {
        authorize_open_path(runtime, &path);
    }
    drop(queue);
    for expired in evicted {
        discard_open_file_selection(runtime, &expired);
    }
    accepted
}

pub fn claim_pending_open_target(
    runtime: &DesktopFileRuntime,
) -> Result<Option<OpenTargetClaim>, AppError> {
    let mut queue = runtime
        .inner
        .open_targets
        .lock()
        .map_err(|_| AppError::poisoned_lock("desktop open target queue"))?;
    let claim = queue.claim();
    if let Some(claim) = &claim {
        authorize_open_path(runtime, &claim.path);
    }
    Ok(claim)
}

pub fn acknowledge_open_target(
    runtime: &DesktopFileRuntime,
    claim_id: &str,
) -> Result<bool, AppError> {
    let mut queue = runtime
        .inner
        .open_targets
        .lock()
        .map_err(|_| AppError::poisoned_lock("desktop open target queue"))?;
    let (path, has_pending_targets) = queue.acknowledge(claim_id);
    if let Some(path) = path {
        discard_open_file_selection(runtime, &path);
    }
    Ok(has_pending_targets)
}

pub fn release_open_target(runtime: &DesktopFileRuntime, claim_id: &str) -> Result<bool, AppError> {
    let mut queue = runtime
        .inner
        .open_targets
        .lock()
        .map_err(|_| AppError::poisoned_lock("desktop open target queue"))?;
    if let Some(path) = queue.release(claim_id) {
        authorize_open_path(runtime, &path);
    }
    Ok(!queue.pending.is_empty())
}

pub fn pick_open_file(
    runtime: &DesktopFileRuntime,
    app: &AppHandle,
) -> Result<Option<OpenFileSelection>, AppError> {
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
    authorize_open_path(runtime, &path);
    let path = path.to_string_lossy().to_string();
    let file_name = file_name_from_path_like(&path, "unknown");
    Ok(Some(OpenFileSelection {
        original_path: path.clone(),
        path,
        file_name,
    }))
}

pub fn read_open_file(runtime: &DesktopFileRuntime, path: &str) -> Result<OpenFileInput, AppError> {
    if !consume_path(
        &runtime.inner.open_paths,
        &normalize_existing_path(Path::new(path)),
    ) {
        return Err(AppError::DocumentStateInvalid(
            "desktop file open path was not selected by the user".to_string(),
        ));
    }
    read_file_trusted(path)
}

pub(crate) fn read_file_trusted(path: &str) -> Result<OpenFileInput, AppError> {
    let metadata = fs::metadata(path).map_err(|e| match e.kind() {
        ErrorKind::NotFound => AppError::FileNotFound(path.to_string()),
        _ => AppError::ReadError(e.to_string()),
    })?;
    validate_input_file_size(metadata.len())?;
    let file = fs::File::open(path).map_err(|e| match e.kind() {
        ErrorKind::NotFound => AppError::FileNotFound(path.to_string()),
        _ => AppError::ReadError(e.to_string()),
    })?;
    Ok(OpenFileInput {
        path: path.to_string(),
        bytes: read_input_bytes(file)?,
        file_name: None,
    })
}

pub fn discard_open_file_selection(runtime: &DesktopFileRuntime, path: &str) {
    revoke_path(
        &runtime.inner.open_paths,
        &normalize_existing_path(Path::new(path)),
    );
}

pub fn pick_save_location(
    runtime: &DesktopFileRuntime,
    app: &AppHandle,
    default_name: &str,
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
    authorize_path(&runtime.inner.save_paths, normalize_target_path(&path));
    Ok(Some(path.to_string_lossy().to_string()))
}

pub fn discard_save_location(runtime: &DesktopFileRuntime, path: &str) {
    revoke_path(
        &runtime.inner.save_paths,
        &normalize_target_path(Path::new(path)),
    );
}

pub(crate) fn ensure_save_path_authorized(
    runtime: &DesktopFileRuntime,
    path: &str,
    current_document_path: &str,
) -> Result<(), AppError> {
    let target = normalize_target_path(Path::new(path));
    if is_current_document_path(&target, current_document_path) {
        return Ok(());
    }
    if consume_path(&runtime.inner.save_paths, &target) {
        return Ok(());
    }
    Err(AppError::DocumentStateInvalid(
        "desktop save target was not selected by the user".to_string(),
    ))
}

pub(crate) struct DesktopExportTarget {
    pub path: PathBuf,
    pub path_string: String,
    pub target_path_or_name: String,
}

pub(crate) fn pick_export_target(
    app: &AppHandle,
    default_name: &str,
) -> Result<Option<DesktopExportTarget>, AppError> {
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
    Ok(Some(DesktopExportTarget {
        path,
        path_string,
        target_path_or_name: output_name_for_selected_target(
            selected_name.as_deref(),
            default_name,
        ),
    }))
}

pub(crate) fn write_export_target(
    target: &DesktopExportTarget,
    bytes: &[u8],
) -> Result<(), AppError> {
    write_file_atomically(&target.path, bytes)
}

fn file_path_to_path_buf(path: FilePath) -> Result<PathBuf, AppError> {
    match path {
        FilePath::Path(path) => Ok(path),
        FilePath::Url(url) => url
            .to_file_path()
            .map_err(|_| AppError::ReadError("Selected desktop file is not a local path".into())),
    }
}

fn is_current_document_path(target: &Path, current_document_path: &str) -> bool {
    !current_document_path.is_empty()
        && normalize_target_path(Path::new(current_document_path)) == target
}

fn authorize_path(paths: &Mutex<PathAuthorizationRegistry>, path: PathBuf) {
    if let Ok(mut paths) = paths.lock() {
        paths.authorize(path);
    }
}

fn consume_path(paths: &Mutex<PathAuthorizationRegistry>, path: &Path) -> bool {
    paths
        .lock()
        .map(|mut paths| paths.consume(path))
        .unwrap_or(false)
}

fn revoke_path(paths: &Mutex<PathAuthorizationRegistry>, path: &Path) {
    if let Ok(mut paths) = paths.lock() {
        paths.revoke(path);
    }
}

impl DesktopFileRuntime {
    #[cfg(test)]
    pub(crate) fn is_same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
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

fn resolve_open_target(target: &str) -> Option<PathBuf> {
    let candidate = if target
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file:"))
    {
        file_uri_to_path(target)?
    } else {
        PathBuf::from(target)
    };
    is_supported_existing_spreadsheet_path(&candidate).then(|| normalize_existing_path(&candidate))
}

fn file_uri_to_path(target: &str) -> Option<PathBuf> {
    let url = tauri::Url::parse(target).ok()?;
    if url.scheme() != "file" {
        return None;
    }
    let decoded = percent_decode(url.path())?;
    let host = url
        .host_str()
        .filter(|host| !host.eq_ignore_ascii_case("localhost"));
    if let Some(host) = host {
        return Some(PathBuf::from(format!("//{host}{decoded}")));
    }
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

fn percent_decode(value: &str) -> Option<String> {
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

    String::from_utf8(decoded).ok()
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
    fn file_uri_parser_accepts_case_insensitive_scheme_and_localhost() {
        assert_eq!(
            file_uri_to_path("FILE://localhost/tmp/report%20final.xlsx").unwrap(),
            PathBuf::from("/tmp/report final.xlsx")
        );
    }

    #[test]
    fn file_association_url_authorizes_the_resolved_spreadsheet_path() {
        let runtime = DesktopFileRuntime::default();
        let path = temp_path("file-association.xlsx");
        std::fs::write(&path, b"").expect("write associated file");
        let target = format!("file://{}", path.to_string_lossy());

        assert!(enqueue_open_target(&runtime, &target));

        assert!(consume_path(
            &runtime.inner.open_paths,
            &normalize_existing_path(&path)
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn launch_targets_are_normalized_claimed_and_acknowledged() {
        let runtime = DesktopFileRuntime::default();
        let path = temp_path("queued-file-association.xlsx");
        std::fs::write(&path, b"").expect("write associated file");
        let target = format!("FILE://localhost{}", path.to_string_lossy());

        assert!(enqueue_open_target(&runtime, &target));

        let claim = claim_pending_open_target(&runtime)
            .expect("claim command")
            .expect("open target claim");
        assert_eq!(
            claim.path,
            normalize_existing_path(&path).to_string_lossy().to_string()
        );
        assert!(claim_pending_open_target(&runtime).unwrap().is_none());
        acknowledge_open_target(&runtime, &claim.claim_id).expect("acknowledge claim");
        assert!(claim_pending_open_target(&runtime).unwrap().is_none());
        assert!(!consume_path(
            &runtime.inner.open_paths,
            &normalize_existing_path(&path)
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn released_launch_target_claim_returns_to_the_front_of_the_queue() {
        let runtime = DesktopFileRuntime::default();
        let path = temp_path("released-file-association.xlsx");
        std::fs::write(&path, b"").expect("write associated file");

        assert!(enqueue_open_target(&runtime, &path.to_string_lossy()));
        let first = claim_pending_open_target(&runtime)
            .expect("claim command")
            .expect("first claim");
        assert!(consume_path(
            &runtime.inner.open_paths,
            &normalize_existing_path(&path)
        ));
        assert!(release_open_target(&runtime, &first.claim_id).expect("release claim"));
        assert!(consume_path(
            &runtime.inner.open_paths,
            &normalize_existing_path(&path)
        ));
        let retried = claim_pending_open_target(&runtime)
            .expect("claim command")
            .expect("retried claim");

        assert_eq!(retried.path, first.path);
        assert_ne!(retried.claim_id, first.claim_id);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn expired_launch_target_claim_is_requeued() {
        let now = Instant::now();
        let mut queue = OpenTargetQueue::default();
        queue.enqueue_at("/tmp/expired.xlsx".to_string(), now);
        let first = queue.claim_at(now).expect("first claim");

        let retried = queue
            .claim_at(now + OPEN_TARGET_CLAIM_TTL)
            .expect("expired claim is requeued");

        assert_eq!(retried.path, first.path);
        assert_ne!(retried.claim_id, first.claim_id);
    }

    #[test]
    fn expired_launch_target_claims_preserve_queue_order() {
        let now = Instant::now();
        let mut queue = OpenTargetQueue::default();
        queue.enqueue_at("/tmp/first-expired.xlsx".to_string(), now);
        queue.enqueue_at("/tmp/second-expired.xlsx".to_string(), now);
        let first = queue.claim_at(now).expect("first claim");
        let second_claimed_at = now + Duration::from_secs(1);
        let second = queue.claim_at(second_claimed_at).expect("second claim");

        let first_retried = queue
            .claim_at(second_claimed_at + OPEN_TARGET_CLAIM_TTL)
            .expect("first expired claim");
        let second_retried = queue
            .claim_at(second_claimed_at + OPEN_TARGET_CLAIM_TTL)
            .expect("second expired claim");

        assert_eq!(first_retried.path, first.path);
        assert_eq!(second_retried.path, second.path);
    }

    #[test]
    fn path_authorizations_are_isolated_by_runtime() {
        let first = DesktopFileRuntime::default();
        let second = DesktopFileRuntime::default();
        let path = temp_path("isolated.xlsx");
        std::fs::write(&path, b"").expect("write selected file");

        authorize_open_path(&first, &path);

        assert!(!consume_path(
            &second.inner.open_paths,
            &normalize_existing_path(&path)
        ));
        assert!(consume_path(
            &first.inner.open_paths,
            &normalize_existing_path(&path)
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn path_authorizations_are_capacity_bounded() {
        let mut registry = PathAuthorizationRegistry::default();
        let now = Instant::now();
        for index in 0..=MAX_AUTHORIZED_PATHS {
            registry.authorize_at(PathBuf::from(format!("/tmp/book-{index}.xlsx")), now);
        }

        assert_eq!(registry.entries.len(), MAX_AUTHORIZED_PATHS);
        assert!(!registry.consume_at(Path::new("/tmp/book-0.xlsx"), now));
        assert!(registry.consume_at(
            Path::new(&format!("/tmp/book-{MAX_AUTHORIZED_PATHS}.xlsx")),
            now
        ));
    }

    #[test]
    fn path_authorizations_expire() {
        let mut registry = PathAuthorizationRegistry::default();
        let now = Instant::now();
        let path = PathBuf::from("/tmp/expiring.xlsx");
        registry.authorize_at(path.clone(), now);

        assert!(!registry.consume_at(&path, now + PATH_AUTHORIZATION_TTL));
        assert!(registry.entries.is_empty());
    }

    #[test]
    fn repeated_authorization_refreshes_the_eviction_order() {
        let mut registry = PathAuthorizationRegistry::default();
        let now = Instant::now();
        let refreshed = PathBuf::from("/tmp/refreshed.xlsx");
        registry.authorize_at(refreshed.clone(), now);
        for index in 0..MAX_AUTHORIZED_PATHS - 1 {
            registry.authorize_at(PathBuf::from(format!("/tmp/book-{index}.xlsx")), now);
        }
        registry.authorize_at(refreshed.clone(), now);
        registry.authorize_at(PathBuf::from("/tmp/newest.xlsx"), now);

        assert!(registry.consume_at(&refreshed, now));
        assert!(!registry.consume_at(Path::new("/tmp/book-0.xlsx"), now));
    }

    #[test]
    fn current_document_path_authorization_is_a_pure_path_comparison() {
        let current = temp_path("current.xlsx");

        assert!(is_current_document_path(
            &normalize_target_path(&current),
            &current.to_string_lossy(),
        ));
        assert!(!is_current_document_path(
            &normalize_target_path(&current),
            "",
        ));
    }
}
