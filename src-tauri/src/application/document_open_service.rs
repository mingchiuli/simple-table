use std::path::PathBuf;

use tauri::AppHandle;
use umya_spreadsheet::Workbook;

use crate::application::prepared_document_repository;
use crate::application::runtime::ApplicationRuntime;
use crate::error::AppError;
use crate::io::codec::reader::{preflight_input_file, read_file_with_workbook_from_preflight};
use crate::io::file_format::{
    default_spreadsheet_extension, file_name_from_path_like, open_extension_from_path_name_or_bytes,
};
use crate::io::open_file_input::OpenFileInput;
use crate::resource_limits::validate_file_data;
use crate::state::editor_state::EditorState;
use crate::types::{FileData, PreparedOpenDocument, SheetData};

pub fn prepare_open_input(
    runtime: &ApplicationRuntime,
    input: OpenFileInput,
) -> Result<PreparedOpenDocument, AppError> {
    let OpenFileInput {
        path,
        bytes,
        file_name,
    } = input;
    let extension = open_extension_from_path_name_or_bytes(&path, file_name.as_deref(), &bytes);
    let preflight = preflight_input_file(&extension, &bytes)?;
    let reservation = runtime.prepared_documents().reserve_for_parse_bytes(
        preflight.estimated_parse_bytes(),
        active_document_resource_bytes(runtime)?,
    )?;
    let resolved_file_name =
        file_name.unwrap_or_else(|| file_name_from_path_like(&path, "unknown"));
    let source_path = PathBuf::from(&path);
    let result =
        read_file_with_workbook_from_preflight(preflight, bytes, path, resolved_file_name)?;

    prepare_editor_state(
        runtime,
        result.file_data,
        result.workbook,
        Some(source_path),
        reservation,
    )
}

#[cfg(desktop)]
pub fn prepare_open_file_desktop(
    runtime: &ApplicationRuntime,
    path: &str,
) -> Result<PreparedOpenDocument, AppError> {
    prepare_open_input(
        runtime,
        crate::io::platform::desktop::read_open_file(runtime.desktop_files(), path)?,
    )
}

#[cfg(desktop)]
pub fn prepare_recent_file_desktop(
    runtime: &ApplicationRuntime,
    app: &AppHandle,
    id: &str,
) -> Result<PreparedOpenDocument, AppError> {
    prepare_open_input(
        runtime,
        crate::io::platform::desktop::read_recent_file(runtime.recent_files(), app, id)?,
    )
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn prepare_open_file_mobile(
    runtime: &ApplicationRuntime,
    app: &AppHandle,
    path: &str,
) -> Result<PreparedOpenDocument, AppError> {
    prepare_open_input(
        runtime,
        crate::io::platform::mobile::read_open_file(runtime.mobile_files(), app, path)?,
    )
}

pub fn prepare_new_file(runtime: &ApplicationRuntime) -> Result<PreparedOpenDocument, AppError> {
    let file_data = blank_file_data();
    validate_file_data(&file_data)?;
    let reservation = runtime
        .prepared_documents()
        .reserve_for_file_data(&file_data, active_document_resource_bytes(runtime)?)?;
    prepare_editor_state(runtime, file_data, None, None, reservation)
}

pub fn abort_prepared_document(runtime: &ApplicationRuntime, token: &str) -> Result<(), AppError> {
    runtime.prepared_documents().abort(token)
}

pub(crate) fn adopt_source_path_if_transient(
    runtime: &ApplicationRuntime,
    source_path: Option<&std::path::Path>,
    file_name: &str,
) -> Result<(), AppError> {
    #[cfg(any(target_os = "android", target_os = "ios", test))]
    if let Some(source_path) = source_path {
        crate::io::managed_documents::adopt_transient_document(
            runtime.mobile_files().managed_documents(),
            runtime.mobile_files().transient_files(),
            source_path,
            file_name,
        )?;
    }

    #[cfg(not(any(target_os = "android", target_os = "ios", test)))]
    let _ = (runtime, source_path, file_name);

    Ok(())
}

fn blank_file_data() -> FileData {
    FileData {
        path: String::new(),
        file_name: format!("untitled.{}", default_spreadsheet_extension()),
        sheets: vec![SheetData {
            name: "Sheet1".to_string(),
            rows: vec![vec![crate::types::CellValue::Null; 5]; 5],
            ..Default::default()
        }],
    }
}

fn prepare_editor_state(
    runtime: &ApplicationRuntime,
    file_data: FileData,
    workbook: Option<Workbook>,
    source_path: Option<PathBuf>,
    reservation: prepared_document_repository::PrepareReservation,
) -> Result<PreparedOpenDocument, AppError> {
    let editor_state = EditorState::with_workbook(file_data, workbook);
    let token = runtime.prepared_documents().replace(
        editor_state,
        source_path,
        reservation,
        active_document_resource_bytes(runtime)?,
    )?;
    Ok(PreparedOpenDocument { token })
}

fn active_document_resource_bytes(runtime: &ApplicationRuntime) -> Result<usize, AppError> {
    let handle = runtime.documents().active_handle()?;
    handle
        .map(|handle| handle.read().map(|state| state.estimated_resource_bytes()))
        .transpose()
        .map(|bytes| bytes.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CellValue;

    #[test]
    fn open_input_detects_extensionless_csv_content() {
        let runtime = ApplicationRuntime::default();
        let prepared = prepare_open_input(
            &runtime,
            OpenFileInput {
                path: "/tmp/imported".to_string(),
                bytes: b"name,score\nalice,42".to_vec(),
                file_name: Some("imported".to_string()),
            },
        )
        .expect("open extensionless csv");
        let response = runtime
            .prepared_documents()
            .take(&prepared.token)
            .expect("prepared document");

        let rows = &response.editor_state.file_data().sheets[0].rows;
        assert_eq!(rows[0][0], CellValue::String("name".to_string()));
        assert_eq!(rows[0][1], CellValue::String("score".to_string()));
        assert_eq!(rows[1][0], CellValue::String("alice".to_string()));
        assert_eq!(rows[1][1], CellValue::Number(42.into()));
    }

    #[test]
    fn new_file_uses_the_backend_owned_blank_template() {
        let runtime = ApplicationRuntime::default();
        let prepared = prepare_new_file(&runtime).expect("init file");
        let response = runtime
            .prepared_documents()
            .take(&prepared.token)
            .expect("prepared document");

        assert_eq!(response.editor_state.file_data().path, "");
        assert_eq!(response.editor_state.file_data().file_name, "untitled.xlsx");
        assert_eq!(response.editor_state.file_data().sheets.len(), 1);
        assert_eq!(response.editor_state.file_data().sheets[0].name, "Sheet1");
        assert_eq!(response.editor_state.file_data().sheets[0].rows.len(), 5);
        assert!(
            response.editor_state.file_data().sheets[0]
                .rows
                .iter()
                .all(|row| row.len() == 5 && row.iter().all(|cell| cell == &CellValue::Null))
        );
    }
}
