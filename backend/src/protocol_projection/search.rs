use crate::domain::SearchOutcome;
use crate::error::AppError;
use crate::resource_limits::MAX_SEARCH_RESPONSE_BYTES;
use crate::types;

use super::size::serialized_json_bytes;

pub(crate) fn search_response(value: SearchOutcome) -> Result<types::SearchResponse, AppError> {
    let mut response = types::SearchResponse {
        results: value
            .results
            .into_iter()
            .map(|result| types::SearchResult {
                sheet_index: result.sheet_index,
                sheet_name: result.sheet_name,
                row: result.row,
                col: result.col,
                value: result.value,
                cell_position: result.cell_position,
            })
            .collect(),
        truncated: value.truncated,
    };

    if serialized_search_response_bytes(&response.results, response.truncated)?
        > MAX_SEARCH_RESPONSE_BYTES
    {
        let mut admitted = 0usize;
        let mut rejected = response.results.len();
        while admitted < rejected {
            let candidate = admitted + (rejected - admitted).div_ceil(2);
            if serialized_search_response_bytes(&response.results[..candidate], true)?
                <= MAX_SEARCH_RESPONSE_BYTES
            {
                admitted = candidate;
            } else {
                rejected = candidate - 1;
            }
        }
        response.results.truncate(admitted);
        response.truncated = true;
    }
    if serialized_json_bytes(&response)? > MAX_SEARCH_RESPONSE_BYTES {
        return Err(AppError::Internal(
            "bounded search response exceeds its transport budget".to_string(),
        ));
    }
    Ok(response)
}

fn serialized_search_response_bytes(
    results: &[types::SearchResult],
    truncated: bool,
) -> Result<usize, AppError> {
    #[derive(serde::Serialize)]
    struct SearchResponseRef<'a> {
        results: &'a [types::SearchResult],
        truncated: bool,
    }

    serialized_json_bytes(&SearchResponseRef { results, truncated })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::SearchHit;

    #[test]
    fn search_projection_enforces_the_serialized_response_budget() {
        let outcome = SearchOutcome {
            results: (0..1_000)
                .map(|row| SearchHit {
                    sheet_index: 0,
                    sheet_name: "Sheet1".to_string(),
                    row,
                    col: 0,
                    value: "\0".repeat(512),
                    cell_position: format!("A{}", row + 1),
                })
                .collect(),
            truncated: false,
        };

        let response = search_response(outcome).expect("bounded search response");

        assert!(response.truncated);
        assert!(serialized_json_bytes(&response).unwrap() <= MAX_SEARCH_RESPONSE_BYTES);
    }
}
