//! Presentation rules for error codes and transient notices.
//!
//! Every code registered in [`crate::protocol::KNOWN_ERROR_CODES`] must map to
//! a specific presentation here; the coverage test below fails otherwise.

use std::time::Duration;

#[cfg(test)]
use crate::protocol::KNOWN_ERROR_CODES;

pub(crate) const TRANSIENT_NOTICE_DURATION: Duration = Duration::from_secs(8);

/// Fallback presentation used when a code is not registered.
const GENERIC_PRESENTATION: ErrorToastPresentation = ErrorToastPresentation {
    title: "Action failed",
    permanent: false,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ErrorToastPresentation {
    pub(crate) title: &'static str,
    pub(crate) permanent: bool,
}

pub(crate) fn error_toast_presentation(code: &str) -> ErrorToastPresentation {
    let (title, permanent) = match code {
        "write_error" | "file_target_unavailable" => ("Could not save file", true),
        "read_error" => ("Could not read file", false),
        "file_not_found" | "not_found" | "mobile_recovery_missing" => ("File unavailable", false),
        "unsupported_format" | "unsupported_attachment" => ("Unsupported file", false),
        "resource_limit_exceeded" => ("Resource limit reached", false),
        "browser_error" | "mobile_file_error" | "android_file_error" => {
            ("File operation failed", false)
        }
        "indexed_db_error"
        | "indexed_db_serialization_error"
        | "memory_store_error"
        | "mobile_recovery_error"
        | "storage_error" => ("Local storage unavailable", true),
        "worker_failed"
        | "worker_disconnected"
        | "worker_start_failed"
        | "worker_protocol_error"
        | "editor_task_failed"
        | "workspace_unavailable" => ("Editor unavailable", true),
        "transaction_rollback_failed" | "document_state_invalid" => {
            ("Workbook state unavailable", true)
        }
        "workbook_patch_failed"
        | "document_changed"
        | "document_closed"
        | "stale_mutation_response" => ("Could not apply changes", false),
        "region_loader_stopped"
        | "region_response_too_large"
        | "region_split_limit"
        | "stale_region_response" => ("Could not load sheet", false),
        "nothing_to_undo"
        | "nothing_to_redo"
        | "cannot_delete_last_sheet"
        | "invalid_sheet_index"
        | "invalid_cell_position"
        | "row_not_found"
        | "no_document"
        | "no_file_loaded"
        | "prepared_document_conflict"
        | "unsupported_workbook_structure" => ("Action unavailable", false),
        "update_error" => ("Update check failed", false),
        "internal" | "invalid_request" | "protocol_error" | "unexpected_reply" => {
            ("Unexpected error", true)
        }
        "client_not_hydrated" => ("Editor unavailable", false),
        _ => (GENERIC_PRESENTATION.title, GENERIC_PRESENTATION.permanent),
    };
    ErrorToastPresentation { title, permanent }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_map_to_user_facing_presentations() {
        for (code, title, permanent) in [
            ("write_error", "Could not save file", true),
            ("read_error", "Could not read file", false),
            ("unsupported_format", "Unsupported file", false),
            ("resource_limit_exceeded", "Resource limit reached", false),
            ("indexed_db_error", "Local storage unavailable", true),
            ("worker_disconnected", "Editor unavailable", true),
            ("document_state_invalid", "Workbook state unavailable", true),
            ("document_changed", "Could not apply changes", false),
            ("region_split_limit", "Could not load sheet", false),
            ("nothing_to_undo", "Action unavailable", false),
            ("update_error", "Update check failed", false),
            ("protocol_error", "Unexpected error", true),
            ("client_not_hydrated", "Editor unavailable", false),
            ("future_error", GENERIC_PRESENTATION.title, false),
        ] {
            assert_eq!(
                error_toast_presentation(code),
                ErrorToastPresentation { title, permanent },
                "unexpected presentation for {code}",
            );
        }
    }

    #[test]
    fn every_known_error_code_has_a_specific_presentation() {
        for code in KNOWN_ERROR_CODES {
            assert_ne!(
                error_toast_presentation(code),
                GENERIC_PRESENTATION,
                "{code} must be presented explicitly"
            );
        }
    }
}
