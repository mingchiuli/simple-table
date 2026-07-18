use crate::error::AppError;

pub(crate) trait DocumentWorkLease: Send {}

pub(crate) trait DocumentWorkBudgetPort: Send + Sync {
    fn reserve_save(
        &self,
        document_id: u64,
        estimated_source_bytes: usize,
    ) -> Result<Box<dyn DocumentWorkLease>, AppError>;
}
