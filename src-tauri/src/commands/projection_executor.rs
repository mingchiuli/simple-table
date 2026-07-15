use std::sync::OnceLock;

use crate::error::AppError;

use super::blocking::BoundedBlockingExecutor;

const MAX_CONCURRENT_PROJECTION_COMMANDS: usize = 2;
const MAX_ADMITTED_PROJECTION_COMMANDS: usize = 8;

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
            "document projection",
            MAX_CONCURRENT_PROJECTION_COMMANDS,
            MAX_ADMITTED_PROJECTION_COMMANDS,
        )
    })
}
