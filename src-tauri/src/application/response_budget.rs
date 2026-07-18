use std::io::Write;

use crate::editor_protocol::MAX_SHEET_REGION_RESPONSE_BYTES;
use crate::error::AppError;
use crate::types::{OpenDocumentResponse, SheetRegionProjectionResponse};

pub(crate) const MAX_REGION_RESPONSE_BYTES: usize = MAX_SHEET_REGION_RESPONSE_BYTES;

pub(crate) fn finalize_open_document_response(
    mut response: OpenDocumentResponse,
) -> OpenDocumentResponse {
    response.initial_region = response
        .initial_region
        .and_then(|region| finalize_region_response(region, MAX_REGION_RESPONSE_BYTES).ok());
    response
}

pub(crate) fn finalize_region_response(
    mut response: SheetRegionProjectionResponse,
    maximum_bytes: usize,
) -> Result<SheetRegionProjectionResponse, AppError> {
    response.estimated_bytes = None;
    let mut estimate = serialized_json_bytes(&response)?;
    for _ in 0..8 {
        response.estimated_bytes = Some(estimate);
        let actual = serialized_json_bytes(&response)?;
        if actual == estimate {
            if actual > maximum_bytes {
                return Err(AppError::RegionResponseTooLarge {
                    estimated_bytes: actual,
                    maximum_bytes,
                });
            }
            return Ok(response);
        }
        estimate = actual;
    }
    Err(AppError::Internal(
        "failed to converge while sizing region response".to_string(),
    ))
}

fn serialized_json_bytes(value: &impl serde::Serialize) -> Result<usize, AppError> {
    let mut counter = CountingWriter::default();
    serde_json::to_writer(&mut counter, value)
        .map_err(|error| AppError::Internal(format!("failed to size region response: {error}")))?;
    Ok(counter.bytes)
}

#[derive(Default)]
struct CountingWriter {
    bytes: usize,
}

impl Write for CountingWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.bytes = self.bytes.saturating_add(buffer.len());
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
