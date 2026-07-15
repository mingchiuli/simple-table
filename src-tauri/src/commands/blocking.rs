use std::sync::{Arc, OnceLock};

use tokio::sync::Semaphore;

use crate::error::AppError;

const MAX_CONCURRENT_BLOCKING_COMMANDS: usize = 2;

pub(crate) async fn run<T, F>(task: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
{
    let permit = semaphore()
        .acquire_owned()
        .await
        .map_err(|_| AppError::Internal("blocking command executor is unavailable".to_string()))?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        task()
    })
    .await
    .map_err(|error| AppError::Internal(format!("blocking command task failed: {error}")))?
}

fn semaphore() -> Arc<Semaphore> {
    static SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
    Arc::clone(SEMAPHORE.get_or_init(|| Arc::new(Semaphore::new(MAX_CONCURRENT_BLOCKING_COMMANDS))))
}
