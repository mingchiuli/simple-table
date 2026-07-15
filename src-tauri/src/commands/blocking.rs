use std::sync::{Arc, OnceLock};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::error::AppError;

const MAX_CONCURRENT_BLOCKING_COMMANDS: usize = 2;
const MAX_ADMITTED_BLOCKING_COMMANDS: usize = 8;

pub(crate) struct BoundedBlockingExecutor {
    execution: Arc<Semaphore>,
    admission: Arc<Semaphore>,
    name: &'static str,
}

impl BoundedBlockingExecutor {
    pub(crate) fn new(name: &'static str, max_concurrent: usize, max_admitted: usize) -> Self {
        assert!(max_concurrent > 0);
        assert!(max_admitted >= max_concurrent);
        Self {
            execution: Arc::new(Semaphore::new(max_concurrent)),
            admission: Arc::new(Semaphore::new(max_admitted)),
            name,
        }
    }

    pub(crate) async fn run<T, F>(&self, task: F) -> Result<T, AppError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, AppError> + Send + 'static,
    {
        let admission = self.try_admit()?;
        let execution = Arc::clone(&self.execution)
            .acquire_owned()
            .await
            .map_err(|_| AppError::Internal(format!("{} executor is unavailable", self.name)))?;
        tauri::async_runtime::spawn_blocking(move || {
            let _admission = admission;
            let _execution = execution;
            task()
        })
        .await
        .map_err(|error| {
            AppError::Internal(format!("{} executor task failed: {error}", self.name))
        })?
    }

    fn try_admit(&self) -> Result<OwnedSemaphorePermit, AppError> {
        Arc::clone(&self.admission)
            .try_acquire_owned()
            .map_err(|_| {
                AppError::ResourceLimitExceeded(format!(
                    "{} executor is at its admission limit",
                    self.name
                ))
            })
    }
}

pub(crate) async fn run<T, F>(task: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
{
    executor().run(task).await
}

fn executor() -> &'static BoundedBlockingExecutor {
    static EXECUTOR: OnceLock<BoundedBlockingExecutor> = OnceLock::new();
    EXECUTOR.get_or_init(|| {
        BoundedBlockingExecutor::new(
            "blocking command",
            MAX_CONCURRENT_BLOCKING_COMMANDS,
            MAX_ADMITTED_BLOCKING_COMMANDS,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::BoundedBlockingExecutor;
    use crate::error::AppError;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn admission_is_bounded_and_released_with_the_permit() {
        let executor = BoundedBlockingExecutor::new("test", 1, 2);
        let first = executor.try_admit().expect("first admission");
        let second = executor.try_admit().expect("second admission");

        assert!(matches!(
            executor.try_admit(),
            Err(AppError::ResourceLimitExceeded(_))
        ));

        drop(first);
        assert!(executor.try_admit().is_ok());
        drop(second);
    }

    #[test]
    fn execution_concurrency_is_bounded_independently_from_admission() {
        tauri::async_runtime::block_on(async {
            let executor = Arc::new(BoundedBlockingExecutor::new("test", 1, 2));
            let active = Arc::new(AtomicUsize::new(0));
            let peak = Arc::new(AtomicUsize::new(0));
            let mut tasks = Vec::new();

            for _ in 0..2 {
                let executor = Arc::clone(&executor);
                let active = Arc::clone(&active);
                let peak = Arc::clone(&peak);
                tasks.push(tauri::async_runtime::spawn(async move {
                    executor
                        .run(move || {
                            let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                            peak.fetch_max(current, Ordering::SeqCst);
                            std::thread::sleep(Duration::from_millis(20));
                            active.fetch_sub(1, Ordering::SeqCst);
                            Ok(())
                        })
                        .await
                }));
            }

            for task in tasks {
                task.await
                    .expect("executor task join")
                    .expect("executor task");
            }
            assert_eq!(peak.load(Ordering::SeqCst), 1);
        });
    }
}
