use crate::error::AppError;

pub const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_XLSX_UNCOMPRESSED_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_XLSX_ARCHIVE_ENTRIES: usize = 20_000;

pub fn validate_input_size(byte_count: usize) -> Result<(), AppError> {
    ensure_input_limit(byte_count as u64)
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

    #[test]
    fn rejects_oversized_input_without_allocating_it() {
        let error = validate_input_size(MAX_INPUT_BYTES + 1).expect_err("oversized input");
        assert!(matches!(error, AppError::ResourceLimitExceeded(_)));
    }
}
