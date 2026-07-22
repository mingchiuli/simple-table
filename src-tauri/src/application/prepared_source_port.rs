use std::path::Path;

use crate::error::AppError;

pub(crate) trait PreparedSourceAdoption: Send {
    fn commit(self: Box<Self>);
}

pub(crate) trait PreparedSourceAdoptionPort: Send + Sync {
    fn begin_adoption(
        &self,
        source_path: Option<&Path>,
        file_name: &str,
    ) -> Result<Box<dyn PreparedSourceAdoption>, AppError>;
}

pub(crate) struct NoopPreparedSourceAdoption;

impl PreparedSourceAdoption for NoopPreparedSourceAdoption {
    fn commit(self: Box<Self>) {}
}
