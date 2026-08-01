use crate::recent::model::{AddRecentFileInput, RecentFileRecord, RecentStorageType};
use crate::types;

pub(crate) fn add_recent_file_input(value: types::AddRecentFileRequest) -> AddRecentFileInput {
    AddRecentFileInput {
        original_path: value.original_path,
        document_id: value.document_id,
        base_revision: value.base_revision,
        path: value.path,
        file_name: value.file_name,
    }
}

pub(crate) fn recent_files(values: Vec<RecentFileRecord>) -> Vec<types::RecentFile> {
    values.into_iter().map(recent_file).collect()
}

pub(crate) fn recent_file(value: RecentFileRecord) -> types::RecentFile {
    types::RecentFile {
        id: value.id,
        path: value.path,
        file_name: value.file_name,
        last_opened: value.last_opened,
        file_size: value.file_size,
        thumbnail: value.thumbnail,
        storage_type: match value.storage_type {
            RecentStorageType::MobileSandboxPath => types::StorageType::MobileSandboxPath,
            RecentStorageType::DesktopPath => types::StorageType::DesktopPath,
        },
        original_path: value.original_path,
    }
}
