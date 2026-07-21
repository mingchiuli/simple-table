use crate::error::AppError;

pub(crate) trait DocumentWorkLease: Send {
    fn set_work_bytes(&mut self, work_bytes: usize) -> Result<(), AppError>;
}

pub(crate) trait DocumentWorkBudgetPort: Send + Sync {
    fn reserve_preparation(
        &self,
        active_document_bytes: usize,
        estimated_work_bytes: usize,
    ) -> Result<Box<dyn DocumentWorkLease>, AppError>;

    fn reserve_save(
        &self,
        document_id: u64,
        active_document_bytes: usize,
        estimated_source_bytes: usize,
    ) -> Result<Box<dyn DocumentWorkLease>, AppError>;
}
