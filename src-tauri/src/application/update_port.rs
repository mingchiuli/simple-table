use std::future::Future;
use std::pin::Pin;

use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpdateRelease {
    pub tag_name: String,
    pub assets: Vec<UpdateReleaseAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpdateReleaseAsset {
    pub name: String,
    pub download_url: String,
}

pub(crate) trait UpdateReleasePort: Send + Sync {
    fn latest_release(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<UpdateRelease, AppError>> + Send + '_>>;
}
