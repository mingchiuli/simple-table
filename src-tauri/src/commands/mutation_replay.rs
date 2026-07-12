use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

use crate::error::AppError;
use crate::types::{EditorMutationResponse, EditorPatch, ResyncRequiredPatch};

const MAX_REPLAY_ENTRIES: usize = 128;
const MAX_REPLAY_BYTES: usize = 4 * 1024 * 1024;
const MAX_COMMAND_ID_BYTES: usize = 128;

#[derive(Clone)]
struct ReplayEntry {
    document_id: u64,
    command_id: String,
    fingerprint: String,
    response: EditorMutationResponse,
    bytes: usize,
}

#[derive(Default)]
struct MutationReplayCache {
    entries: VecDeque<ReplayEntry>,
    bytes: usize,
}

static MUTATION_REPLAYS: OnceLock<Mutex<MutationReplayCache>> = OnceLock::new();

pub(crate) fn run<P: Serialize>(
    document_id: u64,
    base_revision: u64,
    command_id: &str,
    command_name: &str,
    payload: &P,
    execute: impl FnOnce() -> Result<EditorMutationResponse, AppError>,
) -> Result<EditorMutationResponse, AppError> {
    validate_command_id(command_id)?;
    let payload =
        serde_json::to_string(payload).map_err(|error| AppError::Internal(error.to_string()))?;
    let fingerprint = format!("{base_revision}:{command_name}:{payload}");
    let mut cache = replay_cache()
        .lock()
        .map_err(|_| AppError::poisoned_lock("mutation replay cache"))?;

    if let Some(entry) = cache
        .entries
        .iter()
        .find(|entry| entry.document_id == document_id && entry.command_id == command_id)
    {
        if entry.fingerprint != fingerprint {
            return Err(AppError::DocumentStateInvalid(
                "mutation commandId was reused with a different payload".to_string(),
            ));
        }
        return Ok(entry.response.clone());
    }

    let response = execute()?;
    let replay_response = bounded_replay_response(&response);
    let bytes = serde_json::to_vec(&replay_response)
        .map(|value| value.len())
        .unwrap_or(MAX_REPLAY_BYTES.saturating_add(1));
    if bytes <= MAX_REPLAY_BYTES {
        while cache.entries.len() >= MAX_REPLAY_ENTRIES
            || cache.bytes.saturating_add(bytes) > MAX_REPLAY_BYTES
        {
            let Some(expired) = cache.entries.pop_front() else {
                break;
            };
            cache.bytes = cache.bytes.saturating_sub(expired.bytes);
        }
        cache.bytes = cache.bytes.saturating_add(bytes);
        cache.entries.push_back(ReplayEntry {
            document_id,
            command_id: command_id.to_string(),
            fingerprint,
            response: replay_response,
            bytes,
        });
    }
    Ok(response)
}

fn bounded_replay_response(response: &EditorMutationResponse) -> EditorMutationResponse {
    let bytes = serde_json::to_vec(response).map_or(usize::MAX, |value| value.len());
    if bytes <= MAX_REPLAY_BYTES {
        return response.clone();
    }
    let mut compact = response.clone();
    compact.patches = vec![EditorPatch::ResyncRequired {
        patch: ResyncRequiredPatch {
            reason: "mutation response exceeded replay budget".to_string(),
        },
    }];
    compact.sheet_layouts = None;
    compact
}

pub(crate) fn clear_document(document_id: u64) {
    let Ok(mut cache) = replay_cache().lock() else {
        return;
    };
    cache
        .entries
        .retain(|entry| entry.document_id != document_id);
    cache.bytes = cache.entries.iter().map(|entry| entry.bytes).sum();
}

pub(crate) fn get(
    document_id: u64,
    command_id: &str,
) -> Result<Option<EditorMutationResponse>, AppError> {
    validate_command_id(command_id)?;
    let cache = replay_cache()
        .lock()
        .map_err(|_| AppError::poisoned_lock("mutation replay cache"))?;
    Ok(cache
        .entries
        .iter()
        .find(|entry| entry.document_id == document_id && entry.command_id == command_id)
        .map(|entry| entry.response.clone()))
}

fn validate_command_id(command_id: &str) -> Result<(), AppError> {
    if command_id.is_empty() || command_id.len() > MAX_COMMAND_ID_BYTES {
        return Err(AppError::DocumentStateInvalid(
            "mutation commandId must contain between 1 and 128 bytes".to_string(),
        ));
    }
    Ok(())
}

fn replay_cache() -> &'static Mutex<MutationReplayCache> {
    MUTATION_REPLAYS.get_or_init(|| Mutex::new(MutationReplayCache::default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::patch_projector::status_mutation_response;
    use crate::state::editor_state::EditorState;
    use crate::types::FileData;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn replays_successful_mutations_once() {
        let state = EditorState::with_workbook(
            FileData {
                path: String::new(),
                file_name: "book.xlsx".to_string(),
                sheets: Vec::new(),
            },
            None,
        );
        let calls = AtomicUsize::new(0);
        let first = run(91, 0, "command", "set_cell", &(0, 0), || {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(status_mutation_response(&state))
        })
        .expect("first mutation");
        let second = run(91, 0, "command", "set_cell", &(0, 0), || {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(status_mutation_response(&state))
        })
        .expect("replayed mutation");

        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(first.revision, second.revision);
        clear_document(91);
    }
}
