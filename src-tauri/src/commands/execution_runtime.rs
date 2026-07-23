use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::error::AppError;

const MAX_CONCURRENT_COMMANDS: usize = 3;
const MAX_ADMITTED_COMMANDS: usize = 16;
const MAX_CONCURRENT_FILE_COMMANDS: usize = 2;
const MAX_ADMITTED_FILE_COMMANDS: usize = 8;
const MAX_CONCURRENT_MUTATIONS: usize = 1;
const MAX_ADMITTED_MUTATIONS: usize = 8;
const MAX_CONCURRENT_PROJECTIONS: usize = 2;
const MAX_ADMITTED_PROJECTIONS: usize = 8;
const MAX_CONCURRENT_QUERIES: usize = 2;
const MAX_ADMITTED_QUERIES: usize = 8;
const MAX_CONCURRENT_SEARCHES: usize = 1;
const MAX_ADMITTED_SEARCHES: usize = 2;
const MAX_CONCURRENT_RECENT_COMMANDS: usize = 1;
const MAX_ADMITTED_RECENT_COMMANDS: usize = 3;

struct SharedCommandBudget {
    execution: Arc<Semaphore>,
    admission: Arc<Semaphore>,
}

impl SharedCommandBudget {
    fn new(max_concurrent: usize, max_admitted: usize) -> Self {
        assert!(max_concurrent > 0);
        assert!(max_admitted >= max_concurrent);
        Self {
            execution: Arc::new(Semaphore::new(max_concurrent)),
            admission: Arc::new(Semaphore::new(max_admitted)),
        }
    }
}

pub(crate) struct BoundedBlockingExecutor {
    shared: Arc<SharedCommandBudget>,
    execution: Arc<Semaphore>,
    admission: Arc<Semaphore>,
    name: &'static str,
}

impl BoundedBlockingExecutor {
    fn new(
        shared: Arc<SharedCommandBudget>,
        name: &'static str,
        max_concurrent: usize,
        max_admitted: usize,
    ) -> Self {
        assert!(max_concurrent > 0);
        assert!(max_admitted >= max_concurrent);
        Self {
            shared,
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
            .map_err(|_| self.unavailable_error())?;
        let shared_execution = Arc::clone(&self.shared.execution)
            .acquire_owned()
            .await
            .map_err(|_| self.unavailable_error())?;
        tauri::async_runtime::spawn_blocking(move || {
            let _admission = admission;
            let _shared_execution = shared_execution;
            let _execution = execution;
            task()
        })
        .await
        .map_err(|error| {
            AppError::Internal(format!("{} executor task failed: {error}", self.name))
        })?
    }

    pub(crate) async fn run_mapped<T, U, F, M>(&self, task: F, project: M) -> Result<U, AppError>
    where
        T: Send + 'static,
        U: Send + 'static,
        F: FnOnce() -> Result<T, AppError> + Send + 'static,
        M: FnOnce(T) -> U + Send + 'static,
    {
        self.run(move || task().map(project)).await
    }

    fn try_admit(&self) -> Result<CommandAdmission, AppError> {
        let category = Arc::clone(&self.admission)
            .try_acquire_owned()
            .map_err(|_| self.admission_error())?;
        let shared = Arc::clone(&self.shared.admission)
            .try_acquire_owned()
            .map_err(|_| self.admission_error())?;
        Ok(CommandAdmission {
            _shared: shared,
            _category: category,
        })
    }

    fn admission_error(&self) -> AppError {
        AppError::ResourceLimitExceeded(format!("{} executor is at its admission limit", self.name))
    }

    fn unavailable_error(&self) -> AppError {
        AppError::Internal(format!("{} executor is unavailable", self.name))
    }
}

struct CommandAdmission {
    _shared: OwnedSemaphorePermit,
    _category: OwnedSemaphorePermit,
}

#[derive(Clone)]
pub struct CommandExecutionRuntime {
    file: Arc<BoundedBlockingExecutor>,
    mutation: Arc<BoundedBlockingExecutor>,
    projection: Arc<BoundedBlockingExecutor>,
    query: Arc<BoundedBlockingExecutor>,
    search: Arc<BoundedBlockingExecutor>,
    recent: Arc<BoundedBlockingExecutor>,
}

impl Default for CommandExecutionRuntime {
    fn default() -> Self {
        let shared = Arc::new(SharedCommandBudget::new(
            MAX_CONCURRENT_COMMANDS,
            MAX_ADMITTED_COMMANDS,
        ));
        Self {
            file: Arc::new(BoundedBlockingExecutor::new(
                Arc::clone(&shared),
                "file command",
                MAX_CONCURRENT_FILE_COMMANDS,
                MAX_ADMITTED_FILE_COMMANDS,
            )),
            mutation: Arc::new(BoundedBlockingExecutor::new(
                Arc::clone(&shared),
                "document mutation",
                MAX_CONCURRENT_MUTATIONS,
                MAX_ADMITTED_MUTATIONS,
            )),
            projection: Arc::new(BoundedBlockingExecutor::new(
                Arc::clone(&shared),
                "document projection",
                MAX_CONCURRENT_PROJECTIONS,
                MAX_ADMITTED_PROJECTIONS,
            )),
            query: Arc::new(BoundedBlockingExecutor::new(
                Arc::clone(&shared),
                "document query",
                MAX_CONCURRENT_QUERIES,
                MAX_ADMITTED_QUERIES,
            )),
            search: Arc::new(BoundedBlockingExecutor::new(
                Arc::clone(&shared),
                "document search",
                MAX_CONCURRENT_SEARCHES,
                MAX_ADMITTED_SEARCHES,
            )),
            recent: Arc::new(BoundedBlockingExecutor::new(
                shared,
                "recent file",
                MAX_CONCURRENT_RECENT_COMMANDS,
                MAX_ADMITTED_RECENT_COMMANDS,
            )),
        }
    }
}

impl CommandExecutionRuntime {
    pub(crate) fn file(&self) -> &BoundedBlockingExecutor {
        &self.file
    }

    pub(crate) fn mutation(&self) -> &BoundedBlockingExecutor {
        &self.mutation
    }

    pub(crate) fn projection(&self) -> &BoundedBlockingExecutor {
        &self.projection
    }

    pub(crate) fn query(&self) -> &BoundedBlockingExecutor {
        &self.query
    }

    pub(crate) fn search(&self) -> &BoundedBlockingExecutor {
        &self.search
    }

    pub(crate) fn recent(&self) -> &BoundedBlockingExecutor {
        &self.recent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn category_and_shared_admission_are_both_bounded() {
        let shared = Arc::new(SharedCommandBudget::new(1, 2));
        let first = BoundedBlockingExecutor::new(Arc::clone(&shared), "first", 1, 2);
        let second = BoundedBlockingExecutor::new(shared, "second", 1, 2);
        let _first = first.try_admit().expect("first admission");
        let _second = second.try_admit().expect("second admission");

        assert!(matches!(
            first.try_admit(),
            Err(AppError::ResourceLimitExceeded(_))
        ));
    }

    #[test]
    fn shared_execution_budget_caps_different_categories() {
        tauri::async_runtime::block_on(async {
            let shared = Arc::new(SharedCommandBudget::new(1, 4));
            let first = Arc::new(BoundedBlockingExecutor::new(
                Arc::clone(&shared),
                "first",
                1,
                2,
            ));
            let second = Arc::new(BoundedBlockingExecutor::new(shared, "second", 1, 2));
            let active = Arc::new(AtomicUsize::new(0));
            let peak = Arc::new(AtomicUsize::new(0));

            let tasks = [first, second]
                .into_iter()
                .map(|executor| {
                    let active = Arc::clone(&active);
                    let peak = Arc::clone(&peak);
                    tauri::async_runtime::spawn(async move {
                        executor
                            .run(move || {
                                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                                peak.fetch_max(current, Ordering::SeqCst);
                                std::thread::sleep(Duration::from_millis(20));
                                active.fetch_sub(1, Ordering::SeqCst);
                                Ok(())
                            })
                            .await
                    })
                })
                .collect::<Vec<_>>();

            for task in tasks {
                task.await.expect("join").expect("command");
            }
            assert_eq!(peak.load(Ordering::SeqCst), 1);
        });
    }

    #[test]
    fn response_projection_remains_inside_the_execution_permit() {
        tauri::async_runtime::block_on(async {
            let shared = Arc::new(SharedCommandBudget::new(2, 4));
            let executor = Arc::new(BoundedBlockingExecutor::new(shared, "mapped", 1, 2));
            let active = Arc::new(AtomicUsize::new(0));
            let peak = Arc::new(AtomicUsize::new(0));

            let tasks = (0..2)
                .map(|_| {
                    let executor = Arc::clone(&executor);
                    let active = Arc::clone(&active);
                    let peak = Arc::clone(&peak);
                    tauri::async_runtime::spawn(async move {
                        executor
                            .run_mapped(
                                || Ok(()),
                                move |()| {
                                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                                    peak.fetch_max(current, Ordering::SeqCst);
                                    std::thread::sleep(Duration::from_millis(20));
                                    active.fetch_sub(1, Ordering::SeqCst);
                                },
                            )
                            .await
                    })
                })
                .collect::<Vec<_>>();

            for task in tasks {
                task.await.expect("join").expect("command");
            }
            assert_eq!(peak.load(Ordering::SeqCst), 1);
        });
    }

    #[test]
    fn command_execution_runtimes_are_isolated() {
        let first = CommandExecutionRuntime::default();
        let second = CommandExecutionRuntime::default();
        assert!(!Arc::ptr_eq(&first.file, &second.file));
        assert!(!Arc::ptr_eq(&first.mutation, &second.mutation));
        assert!(!Arc::ptr_eq(&first.query, &second.query));
    }
}
