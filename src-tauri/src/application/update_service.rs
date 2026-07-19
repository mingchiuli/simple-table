use std::sync::Arc;

use semver::Version;

use crate::application::update_port::{UpdateRelease, UpdateReleasePort};
use crate::error::AppError;
use crate::projection_model::MobileUpdateSnapshot;

const RELEASE_URL_PREFIX: &str = "https://github.com/mingchiuli/simple-table/releases/tag/";
const APK_URL_PREFIX: &str = "https://github.com/mingchiuli/simple-table/releases/download/";
const MAX_VERSION_BYTES: usize = 128;

#[derive(Clone)]
pub struct UpdateService {
    releases: Arc<dyn UpdateReleasePort>,
}

impl UpdateService {
    pub(crate) fn new(releases: Arc<dyn UpdateReleasePort>) -> Self {
        Self { releases }
    }

    pub async fn check_mobile(
        &self,
        current_version: &str,
    ) -> Result<Option<MobileUpdateSnapshot>, AppError> {
        let current_version = parse_version(current_version, "current app version")?;
        let release = self.releases.latest_release().await?;
        let release_version = parse_version(&release.tag_name, "release tag")?;
        if release_version <= current_version {
            return Ok(None);
        }
        Ok(Some(project_update(release, release_version)))
    }

    #[cfg(test)]
    pub(crate) fn is_isolated_from(&self, other: &Self) -> bool {
        !Arc::ptr_eq(&self.releases, &other.releases)
    }
}

fn parse_version(value: &str, label: &str) -> Result<Version, AppError> {
    if value.is_empty() || value.len() > MAX_VERSION_BYTES {
        return Err(AppError::UpdateError(format!(
            "Invalid {label}: expected between 1 and {MAX_VERSION_BYTES} bytes"
        )));
    }
    let normalized = value.strip_prefix('v').unwrap_or(value);
    Version::parse(normalized)
        .map_err(|error| AppError::UpdateError(format!("invalid {label} '{value}': {error}")))
}

fn project_update(release: UpdateRelease, version: Version) -> MobileUpdateSnapshot {
    let apk_url = release
        .assets
        .iter()
        .find(|asset| {
            asset.name.ends_with(".apk") && asset.download_url.starts_with(APK_URL_PREFIX)
        })
        .map(|asset| asset.download_url.clone());
    MobileUpdateSnapshot {
        version: version.to_string(),
        release_url: format!("{RELEASE_URL_PREFIX}{}", release.tag_name),
        apk_url,
        tag_name: release.tag_name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::update_port::UpdateReleaseAsset;

    struct FixedReleasePort(UpdateRelease);

    impl UpdateReleasePort for FixedReleasePort {
        fn latest_release(
            &self,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<UpdateRelease, AppError>> + Send + '_>,
        > {
            let release = self.0.clone();
            Box::pin(async move { Ok(release) })
        }
    }

    #[test]
    fn service_queries_the_port_and_returns_an_internal_snapshot() {
        tauri::async_runtime::block_on(async {
            let service = UpdateService::new(Arc::new(FixedReleasePort(UpdateRelease {
                tag_name: "v1.2.3".to_string(),
                assets: Vec::new(),
            })));

            let update = service
                .check_mobile("1.2.2")
                .await
                .expect("update check")
                .expect("newer release");

            assert_eq!(update.version, "1.2.3");
            assert_eq!(update.tag_name, "v1.2.3");
        });
    }

    #[test]
    fn semver_comparison_handles_prereleases() {
        let current = parse_version("1.2.3-beta.1", "current").unwrap();
        let release = parse_version("v1.2.3", "release").unwrap();

        assert!(release > current);
        assert!(parse_version("1.2.2", "release").unwrap() < current);
    }

    #[test]
    fn invalid_versions_are_rejected_instead_of_partially_parsed() {
        assert!(parse_version("1.2.invalid", "release").is_err());
        assert!(parse_version("", "release").is_err());
    }

    #[test]
    fn only_repository_apk_assets_are_exposed() {
        let update = project_update(
            UpdateRelease {
                tag_name: "v1.2.3".to_string(),
                assets: vec![
                    UpdateReleaseAsset {
                        name: "unsafe.apk".to_string(),
                        download_url: "https://example.com/unsafe.apk".to_string(),
                    },
                    UpdateReleaseAsset {
                        name: "simple-table.apk".to_string(),
                        download_url: "https://github.com/mingchiuli/simple-table/releases/download/v1.2.3/simple-table.apk".to_string(),
                    },
                ],
            },
            Version::parse("1.2.3").unwrap(),
        );

        assert_eq!(
            update.apk_url.as_deref(),
            Some(
                "https://github.com/mingchiuli/simple-table/releases/download/v1.2.3/simple-table.apk"
            )
        );
    }
}
