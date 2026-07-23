#[cfg(any(desktop, target_os = "android", target_os = "ios"))]
use crate::io::open_file_input::OpenFileSelection;
#[cfg(desktop)]
use crate::io::platform::desktop::OpenTargetClaim;
#[cfg(any(desktop, target_os = "android", target_os = "ios"))]
use crate::types;

#[cfg(desktop)]
pub(crate) fn desktop_open_file_info(value: OpenFileSelection) -> types::DesktopOpenFileInfo {
    types::DesktopOpenFileInfo {
        path: value.path,
        file_name: value.file_name,
    }
}

#[cfg(desktop)]
pub(crate) fn desktop_open_target_claim(value: OpenTargetClaim) -> types::DesktopOpenTargetClaim {
    types::DesktopOpenTargetClaim {
        claim_id: value.claim_id,
        path: value.path,
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub(crate) fn picked_file_info(value: OpenFileSelection) -> types::PickedFileInfo {
    types::PickedFileInfo {
        path: value.path,
        original_path: value.original_path,
        file_name: value.file_name,
    }
}

#[cfg(all(test, desktop))]
mod tests {
    use super::*;

    #[test]
    fn desktop_file_selection_projection_drops_internal_source_metadata() {
        let projected = desktop_open_file_info(OpenFileSelection {
            path: "/tmp/imported.xlsx".to_string(),
            original_path: "file:///external/book.xlsx".to_string(),
            file_name: "book.xlsx".to_string(),
        });

        assert_eq!(projected.path, "/tmp/imported.xlsx");
        assert_eq!(projected.file_name, "book.xlsx");
    }
}
