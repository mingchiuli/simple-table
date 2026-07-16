use crate::error::AppError;
use std::fs::{self, DirEntry, File};
use std::io::Read;
use std::path::Path;

pub(crate) const MAX_STORAGE_DIRECTORY_ENTRIES: usize = 1_024;
pub(crate) const MAX_MARKER_BYTES: usize = 16 * 1024;
pub(crate) const MAX_MARKER_FIELD_BYTES: usize = 1_024;

pub(crate) fn bounded_directory_entries(
    directory: &Path,
    label: &str,
) -> Result<Vec<DirEntry>, AppError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(AppError::ReadError(format!(
                "Failed to inspect {label} directory: {error}"
            )));
        }
    };

    let mut collected = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            AppError::ReadError(format!(
                "Failed to inspect {label} directory entry: {error}"
            ))
        })?;
        if collected.len() >= MAX_STORAGE_DIRECTORY_ENTRIES {
            return Err(AppError::ResourceLimitExceeded(format!(
                "{label} directory contains more than {MAX_STORAGE_DIRECTORY_ENTRIES} entries"
            )));
        }
        collected.push(entry);
    }
    Ok(collected)
}

pub(crate) fn read_marker_bytes(path: &Path, label: &str) -> Result<Vec<u8>, AppError> {
    let file = File::open(path)
        .map_err(|error| AppError::ReadError(format!("Failed to open {label}: {error}")))?;
    let mut bytes = Vec::new();
    file.take((MAX_MARKER_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| AppError::ReadError(format!("Failed to read {label}: {error}")))?;
    if bytes.len() > MAX_MARKER_BYTES {
        return Err(AppError::ResourceLimitExceeded(format!(
            "{label} exceeds the maximum size of {MAX_MARKER_BYTES} bytes"
        )));
    }
    Ok(bytes)
}

pub(crate) fn validate_marker_field(label: &str, value: &str) -> Result<(), AppError> {
    if value.is_empty() || value.len() > MAX_MARKER_FIELD_BYTES {
        return Err(AppError::ResourceLimitExceeded(format!(
            "Invalid marker {label}: expected between 1 and {MAX_MARKER_FIELD_BYTES} bytes"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    #[test]
    fn marker_reads_stop_at_the_byte_limit() {
        let path = temp_path("oversized-marker");
        let mut file = File::create(&path).expect("marker file");
        file.write_all(&vec![b'x'; MAX_MARKER_BYTES + 1])
            .expect("marker bytes");

        assert!(matches!(
            read_marker_bytes(&path, "test marker"),
            Err(AppError::ResourceLimitExceeded(_))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn directory_scans_stop_at_the_entry_limit() {
        let directory = temp_path("bounded-directory");
        fs::create_dir_all(&directory).expect("directory");
        for index in 0..=MAX_STORAGE_DIRECTORY_ENTRIES {
            fs::write(directory.join(index.to_string()), []).expect("entry");
        }

        assert!(matches!(
            bounded_directory_entries(&directory, "test"),
            Err(AppError::ResourceLimitExceeded(_))
        ));
        let _ = fs::remove_dir_all(directory);
    }

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("simple-table-{label}-{}", uuid::Uuid::new_v4()))
    }
}
