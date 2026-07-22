use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum RecentStorageType {
    MobileSandboxPath,
    #[default]
    DesktopPath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecentFileRecord {
    pub id: String,
    pub path: String,
    pub file_name: String,
    pub last_opened: i64,
    pub file_size: i64,
    pub thumbnail: Option<String>,
    pub storage_type: RecentStorageType,
    pub original_path: Option<String>,
}

impl RecentFileRecord {
    pub(crate) fn new(path: String, file_name: String, file_size: i64) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            path,
            file_name,
            last_opened: timestamp_millis(SystemTime::now()),
            file_size,
            thumbnail: None,
            storage_type: RecentStorageType::default(),
            original_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AddRecentFileInput {
    pub original_path: Option<String>,
    pub document_id: u64,
    pub base_revision: u64,
}

fn timestamp_millis(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn timestamp_millis_clamps_times_before_unix_epoch() {
        assert_eq!(timestamp_millis(UNIX_EPOCH - Duration::from_millis(1)), 0);
    }

    #[test]
    fn timestamp_millis_clamps_values_above_i64_range() {
        let far_future = UNIX_EPOCH + Duration::from_millis(i64::MAX as u64 + 1);

        assert_eq!(timestamp_millis(far_future), i64::MAX);
    }
}
