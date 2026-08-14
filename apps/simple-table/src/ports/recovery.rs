use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use crate::protocol::AppErrorDto;

pub type RecoveryFuture<T> = Pin<Box<dyn Future<Output = T> + 'static>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryDocument {
    pub name: String,
    pub bytes: Vec<u8>,
    pub updated_at_ms: u64,
}

pub trait RecoveryPort {
    fn load(&self) -> RecoveryFuture<Result<Option<RecoveryDocument>, AppErrorDto>>;
    fn checkpoint(&self, name: String, bytes: Vec<u8>) -> RecoveryFuture<Result<(), AppErrorDto>>;
    fn clear(&self) -> RecoveryFuture<Result<(), AppErrorDto>>;
}

pub fn platform_recovery_port() -> Rc<dyn RecoveryPort> {
    #[cfg(target_os = "android")]
    return Rc::new(android::AndroidRecoveryPort);

    #[cfg(not(target_os = "android"))]
    Rc::new(UnavailableRecoveryPort)
}

#[cfg(not(target_os = "android"))]
struct UnavailableRecoveryPort;

#[cfg(not(target_os = "android"))]
impl RecoveryPort for UnavailableRecoveryPort {
    fn load(&self) -> RecoveryFuture<Result<Option<RecoveryDocument>, AppErrorDto>> {
        Box::pin(async { Ok(None) })
    }

    fn checkpoint(
        &self,
        _name: String,
        _bytes: Vec<u8>,
    ) -> RecoveryFuture<Result<(), AppErrorDto>> {
        Box::pin(async { Ok(()) })
    }

    fn clear(&self) -> RecoveryFuture<Result<(), AppErrorDto>> {
        Box::pin(async { Ok(()) })
    }
}

#[cfg(target_os = "android")]
mod android {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde::{Deserialize, Serialize};

    use super::*;

    const RECOVERY_VERSION: u16 = 1;
    const RECOVERY_DIRECTORY: &str = "recovery";
    const RECOVERY_DATA: &str = "workbook.bin";
    const RECOVERY_METADATA: &str = "metadata.json";

    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RecoveryMetadata {
        version: u16,
        name: String,
        updated_at_ms: u64,
    }

    pub struct AndroidRecoveryPort;

    impl RecoveryPort for AndroidRecoveryPort {
        fn load(&self) -> RecoveryFuture<Result<Option<RecoveryDocument>, AppErrorDto>> {
            Box::pin(async {
                tokio::task::spawn_blocking(load_recovery)
                    .await
                    .map_err(task_error)?
            })
        }

        fn checkpoint(
            &self,
            name: String,
            bytes: Vec<u8>,
        ) -> RecoveryFuture<Result<(), AppErrorDto>> {
            Box::pin(async move {
                tokio::task::spawn_blocking(move || write_recovery(name, bytes))
                    .await
                    .map_err(task_error)?
            })
        }

        fn clear(&self) -> RecoveryFuture<Result<(), AppErrorDto>> {
            Box::pin(async {
                tokio::task::spawn_blocking(clear_recovery)
                    .await
                    .map_err(task_error)?
            })
        }
    }

    fn load_recovery() -> Result<Option<RecoveryDocument>, AppErrorDto> {
        let directory = recovery_directory()?;
        let metadata_path = directory.join(RECOVERY_METADATA);
        let data_path = directory.join(RECOVERY_DATA);
        if !metadata_path.exists() || !data_path.exists() {
            return Ok(None);
        }
        let metadata = std::fs::read(&metadata_path).map_err(io_error)?;
        let metadata: RecoveryMetadata = serde_json::from_slice(&metadata).map_err(io_error)?;
        if metadata.version != RECOVERY_VERSION {
            return Err(recovery_error("unsupported mobile recovery version"));
        }
        let bytes = std::fs::read(data_path).map_err(io_error)?;
        Ok(Some(RecoveryDocument {
            name: metadata.name,
            bytes,
            updated_at_ms: metadata.updated_at_ms,
        }))
    }

    fn write_recovery(name: String, bytes: Vec<u8>) -> Result<(), AppErrorDto> {
        let directory = recovery_directory()?;
        std::fs::create_dir_all(&directory).map_err(io_error)?;
        write_atomically(&directory.join(RECOVERY_DATA), &bytes)?;
        let metadata = RecoveryMetadata {
            version: RECOVERY_VERSION,
            name,
            updated_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
        };
        let metadata = serde_json::to_vec(&metadata).map_err(io_error)?;
        write_atomically(&directory.join(RECOVERY_METADATA), &metadata)
    }

    fn clear_recovery() -> Result<(), AppErrorDto> {
        let directory = recovery_directory()?;
        remove_if_present(&directory.join(RECOVERY_METADATA))?;
        remove_if_present(&directory.join(RECOVERY_DATA))
    }

    fn recovery_directory() -> Result<PathBuf, AppErrorDto> {
        crate::ports::android::app_files_dir().map(|path| path.join(RECOVERY_DIRECTORY))
    }

    fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), AppErrorDto> {
        simple_table_engine::write_native_file_atomically(path, bytes)
    }

    fn remove_if_present(path: &Path) -> Result<(), AppErrorDto> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_error(error)),
        }
    }

    fn io_error(error: impl std::fmt::Display) -> AppErrorDto {
        recovery_error(error.to_string())
    }

    fn task_error(error: tokio::task::JoinError) -> AppErrorDto {
        recovery_error(format!("mobile recovery task failed: {error}"))
    }

    fn recovery_error(message: impl Into<String>) -> AppErrorDto {
        AppErrorDto {
            code: "mobile_recovery_error".to_string(),
            message: message.into(),
        }
    }
}
