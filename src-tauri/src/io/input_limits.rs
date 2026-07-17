use std::io::Read;

use crate::error::AppError;

pub const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_XLSX_UNCOMPRESSED_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_XLSX_ARCHIVE_ENTRIES: usize = 20_000;

pub fn validate_input_size(byte_count: usize) -> Result<(), AppError> {
    ensure_input_limit(byte_count as u64)
}

pub fn validate_input_file_size(byte_count: u64) -> Result<(), AppError> {
    ensure_input_limit(byte_count)
}

pub fn read_input_bytes(reader: impl Read) -> Result<Vec<u8>, AppError> {
    read_to_end_with_limit(reader, MAX_INPUT_BYTES)
}

fn read_to_end_with_limit(reader: impl Read, maximum_bytes: usize) -> Result<Vec<u8>, AppError> {
    let read_limit = (maximum_bytes as u64).saturating_add(1);
    let mut bytes = Vec::new();
    reader
        .take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|error| AppError::ReadError(error.to_string()))?;
    if bytes.len() > maximum_bytes {
        return Err(AppError::ResourceLimitExceeded(format!(
            "file bytes is at least {}, maximum is {maximum_bytes}",
            bytes.len()
        )));
    }
    Ok(bytes)
}

fn ensure_input_limit(byte_count: u64) -> Result<(), AppError> {
    if byte_count > MAX_INPUT_BYTES as u64 {
        return Err(AppError::ResourceLimitExceeded(format!(
            "file bytes is {byte_count}, maximum is {MAX_INPUT_BYTES}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn rejects_oversized_input_without_allocating_it() {
        let error = validate_input_size(MAX_INPUT_BYTES + 1).expect_err("oversized input");
        assert!(matches!(error, AppError::ResourceLimitExceeded(_)));

        let error = validate_input_file_size(MAX_INPUT_BYTES as u64 + 1)
            .expect_err("oversized input metadata");
        assert!(matches!(error, AppError::ResourceLimitExceeded(_)));
    }

    #[test]
    fn bounded_input_read_rejects_data_before_reading_past_the_limit() {
        let error =
            read_to_end_with_limit(Cursor::new(vec![0; 9]), 8).expect_err("oversized input stream");
        assert!(matches!(error, AppError::ResourceLimitExceeded(_)));
    }

    #[test]
    fn bounded_input_read_accepts_data_at_the_limit() {
        let bytes =
            read_to_end_with_limit(Cursor::new(vec![1; 8]), 8).expect("bounded input stream");
        assert_eq!(bytes, vec![1; 8]);
    }
}
