use std::sync::OnceLock;

use crate::error::AppError;

use super::blocking::BoundedBlockingExecutor;

const MAX_CONCURRENT_SEARCHES: usize = 1;
const MAX_ADMITTED_SEARCHES: usize = 2;

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
            "document search",
            MAX_CONCURRENT_SEARCHES,
            MAX_ADMITTED_SEARCHES,
        )
    })
}
