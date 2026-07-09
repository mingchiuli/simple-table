use crate::error::AppError;
use crate::io::codec::reader::read_file_with_workbook_from_bytes;
use crate::io::file_format::{
    default_spreadsheet_extension, export_extensions, extension_of, file_name_from_path_like,
    is_xlsx_extension, open_extension_from_path_name_or_bytes, spreadsheet_format_options,
    supported_extension_from_name,
};
use crate::ops::index_ops::{cancel_index_jobs_for_document, spawn_rebuild_all_sheets_index};
use crate::ops::patch_projector::editor_state_info;
use crate::state::{
    active_document_store,
    editor_state::{EditorState, SaveCommitLease},
    state::EditorSessionInfo,
};
use crate::types::{
    DocumentCapabilities, FileData, NativeSavePlan, OpenDocumentResponse, SavedDocumentResponse,
    SpreadsheetFormatOptions, WorkbookCapabilities,
};
use umya_spreadsheet::Workbook;

/// 从已读取的文件字节打开文档，并初始化编辑器状态
pub fn open_from_bytes(
    path: String,
    bytes: Vec<u8>,
    file_name: Option<String>,
) -> Result<OpenDocumentResponse, AppError> {
    let extension = open_extension_from_path_name_or_bytes(&path, file_name.as_deref(), &bytes);

    // 如果调用方已经解析出文件名，优先使用；否则从路径解析
    let resolved_file_name =
        file_name.unwrap_or_else(|| file_name_from_path_like(&path, "unknown"));

    // 传入 path 到 reader，同时保留 Excel 原始 Workbook 用于后续无损 patch 保存。
    let result = read_file_with_workbook_from_bytes(&extension, bytes, path, resolved_file_name)?;

    // 初始化编辑器状态
    let document = init_editor_state(result.file_data, result.workbook)?;

    Ok(document)
}

/// 初始化编辑器状态（用于新建文件）
pub fn init_file(file_data: FileData) -> Result<OpenDocumentResponse, AppError> {
    init_editor_state(file_data, None)
}

pub fn generate_current_file_bytes_for_target(
    target_path_or_name: &str,
) -> Result<(String, Vec<u8>), AppError> {
    let registry = active_document_store();
    let snapshot = {
        let registry_guard = registry
            .read()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        registry_guard
            .active()
            .ok_or(AppError::NoFileLoaded)?
            .save_snapshot_for_target(target_path_or_name)?
    };
    snapshot.generate_file_bytes_for_target(target_path_or_name)
}

pub struct PreparedDocumentSave {
    pub document_id: u64,
    pub revision: u64,
    pub output_name: String,
    pub bytes: Vec<u8>,
    pub finish_without_reparse: bool,
}

pub fn prepare_current_file_save(
    target_path_or_name: &str,
) -> Result<PreparedDocumentSave, AppError> {
    let registry = active_document_store();
    let document_id;
    let revision;
    let snapshot;
    {
        let registry_guard = registry
            .read()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        let editor_state = registry_guard.active().ok_or(AppError::NoFileLoaded)?;
        if editor_state.has_save_commit_in_progress() {
            return Err(AppError::DocumentStateInvalid(
                "save is already in progress".to_string(),
            ));
        }
        document_id = editor_state.document_id();
        revision = editor_state.revision();
        snapshot = editor_state.save_snapshot_for_target(target_path_or_name)?;
    }

    let (output_name, bytes) = match snapshot.generate_file_bytes_for_target(target_path_or_name) {
        Ok(result) => result,
        Err(error) => return Err(error),
    };
    let target_extension = extension_of(&output_name)
        .or_else(|| extension_of(target_path_or_name))
        .unwrap_or_else(default_extension_string);
    Ok(PreparedDocumentSave {
        document_id,
        revision,
        output_name,
        bytes,
        finish_without_reparse: is_xlsx_extension(&target_extension) && snapshot.is_excel_backed(),
    })
}

pub fn abort_prepared_file_save(prepared: &PreparedDocumentSave) {
    let _ = prepared;
}

pub fn commit_current_file_save<F>(
    path: String,
    prepared: PreparedDocumentSave,
    commit_write: F,
) -> Result<SavedDocumentResponse, AppError>
where
    F: FnOnce() -> Result<(), AppError>,
{
    let document_id_token = prepared.document_id;
    let revision_token = prepared.revision;
    let output_name = prepared.output_name;
    let finish_without_reparse = prepared.finish_without_reparse;
    let bytes = prepared.bytes;
    let extension = extension_of(&output_name)
        .or_else(|| extension_of(&path))
        .unwrap_or_else(default_extension_string);
    if finish_without_reparse {
        return commit_current_file_save_without_reparse(
            path,
            document_id_token,
            revision_token,
            output_name,
            extension,
            commit_write,
        );
    }
    let result =
        match read_file_with_workbook_from_bytes(&extension, bytes, path.clone(), output_name) {
            Ok(result) => result,
            Err(error) => return Err(error),
        };
    let registry = active_document_store();
    let (lease, clear_history) = match begin_prepared_save_commit(
        &registry,
        document_id_token,
        revision_token,
        |editor_state| {
            let current_extension = extension_of(&editor_state.file_data().file_name)
                .or_else(|| extension_of(&editor_state.file_data().path));
            let saved_extension = extension_of(&result.file_data.file_name)
                .or_else(|| extension_of(&result.file_data.path));
            current_extension != saved_extension
        },
    ) {
        Ok(result) => result,
        Err(error) => return Err(error),
    };

    if let Err(error) = commit_write() {
        abort_save_commit(&registry, document_id_token, lease);
        return Err(error);
    }

    let document_id;
    let response;
    {
        let mut registry_guard = registry
            .write()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        let editor_state = registry_guard.get_mut(document_id_token).ok_or_else(|| {
            AppError::DocumentStateInvalid(
                "active document changed while save was in progress".to_string(),
            )
        })?;
        editor_state.finish_save_commit(lease, result.file_data, result.workbook, clear_history)?;
        document_id = editor_state.document_id();
        response = SavedDocumentResponse {
            file_data: editor_state.file_data().clone(),
            editor_session: editor_session_info(editor_state),
        };
    }
    spawn_rebuild_all_sheets_index(&registry, document_id);
    Ok(response)
}

fn begin_prepared_save_commit<F>(
    registry: &std::sync::Arc<std::sync::RwLock<crate::state::state::ActiveDocumentStore>>,
    document_id: u64,
    revision: u64,
    clear_history: F,
) -> Result<(SaveCommitLease, bool), AppError>
where
    F: FnOnce(&EditorState) -> bool,
{
    let mut registry_guard = registry
        .write()
        .map_err(|_| AppError::poisoned_lock("document registry"))?;
    let editor_state = registry_guard.get_mut(document_id).ok_or_else(|| {
        AppError::DocumentStateInvalid(
            "active document changed while save was in progress".to_string(),
        )
    })?;
    ensure_editor_matches_prepared_save(editor_state, document_id, revision)?;
    let clear_history = clear_history(editor_state);
    let lease = editor_state.begin_save_commit(document_id, revision)?;
    Ok((lease, clear_history))
}

fn commit_current_file_save_without_reparse<F>(
    path: String,
    document_id_token: u64,
    revision_token: u64,
    output_name: String,
    saved_extension: String,
    commit_write: F,
) -> Result<SavedDocumentResponse, AppError>
where
    F: FnOnce() -> Result<(), AppError>,
{
    let registry = active_document_store();
    let (lease, clear_history) = match begin_prepared_save_commit(
        &registry,
        document_id_token,
        revision_token,
        |editor_state| {
            let current_extension = extension_of(&editor_state.file_data().file_name)
                .or_else(|| extension_of(&editor_state.file_data().path));
            current_extension.as_deref() != Some(saved_extension.as_str())
        },
    ) {
        Ok(result) => result,
        Err(error) => return Err(error),
    };

    if let Err(error) = commit_write() {
        abort_save_commit(&registry, document_id_token, lease);
        return Err(error);
    }

    let response;
    {
        let mut registry_guard = registry
            .write()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        let editor_state = registry_guard.get_mut(document_id_token).ok_or_else(|| {
            AppError::DocumentStateInvalid(
                "active document changed while save was in progress".to_string(),
            )
        })?;
        editor_state.finish_save_commit_without_reparse(lease, path, output_name, clear_history)?;
        response = SavedDocumentResponse {
            file_data: editor_state.file_data().clone(),
            editor_session: editor_session_info(editor_state),
        };
    }
    Ok(response)
}

fn ensure_editor_matches_prepared_save(
    editor_state: &EditorState,
    document_id: u64,
    revision: u64,
) -> Result<(), AppError> {
    if editor_state.document_id() == document_id && editor_state.revision() == revision {
        Ok(())
    } else {
        Err(AppError::DocumentStateInvalid(
            "document changed while save was being prepared; please save again".to_string(),
        ))
    }
}

fn abort_save_commit(
    registry: &std::sync::Arc<std::sync::RwLock<crate::state::state::ActiveDocumentStore>>,
    document_id: u64,
    lease: crate::state::editor_state::SaveCommitLease,
) {
    if let Ok(mut registry_guard) = registry.write()
        && let Some(editor_state) = registry_guard.get_mut(document_id)
    {
        editor_state.abort_save_commit(lease);
    }
}

pub fn current_file_data() -> Result<FileData, AppError> {
    let registry = active_document_store();
    let registry_guard = registry
        .read()
        .map_err(|_| AppError::poisoned_lock("document registry"))?;

    registry_guard
        .active()
        .map(|editor_state| editor_state.file_data().clone())
        .ok_or(AppError::NoFileLoaded)
}

pub fn close_current_document() -> Result<(), AppError> {
    let registry = active_document_store();
    let closed_document_id = {
        let mut registry_guard = registry
            .write()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        if registry_guard
            .active()
            .is_some_and(EditorState::has_save_commit_in_progress)
        {
            return Err(AppError::DocumentStateInvalid(
                "cannot close document while save is in progress".to_string(),
            ));
        }
        registry_guard.close_active()?
    };
    if let Some(document_id) = closed_document_id {
        cancel_index_jobs_for_document(document_id);
    }
    Ok(())
}

pub fn update_current_file_identity(path: String, file_name: String) -> Result<(), AppError> {
    let registry = active_document_store();
    let mut registry_guard = registry
        .write()
        .map_err(|_| AppError::poisoned_lock("document registry"))?;

    let editor_state = registry_guard.active_mut().ok_or(AppError::NoFileLoaded)?;
    if editor_state.has_save_commit_in_progress() {
        return Err(AppError::DocumentStateInvalid(
            "cannot update file identity while save is in progress".to_string(),
        ));
    }
    editor_state.update_identity(path, file_name);
    Ok(())
}

pub fn document_capabilities(file_name: &str, current_path: Option<&str>) -> DocumentCapabilities {
    let source_name = current_path.unwrap_or(file_name);
    let source_format = document_format(source_name)
        .or_else(|| document_format(file_name))
        .unwrap_or_else(default_extension_string);
    let native_extension = native_save_extension(source_name);
    let native_save_allowed = native_extension.is_some();
    let export_extension = export_extension(file_name).unwrap_or_else(|| source_format.clone());
    let export_formats = export_formats_for(&source_format);

    DocumentCapabilities {
        source_format,
        can_save_original: native_save_allowed,
        native_save_format: native_extension.clone(),
        export_formats,
        requires_save_as_for_native_save: native_extension.is_none(),
        native_save_extension: native_extension,
        export_extension,
        workbook: active_workbook_capabilities(file_name, current_path, native_save_allowed),
    }
}

pub fn native_save_plan(target_path_or_name: &str) -> NativeSavePlan {
    let source_format =
        document_format(target_path_or_name).unwrap_or_else(default_extension_string);
    let native_extension = native_save_extension(target_path_or_name);
    let native_save_allowed = native_extension.is_some();
    let export_extension =
        export_extension(target_path_or_name).unwrap_or_else(|| source_format.clone());
    let export_formats = export_formats_for(&source_format);
    let workbook = active_workbook_capabilities_for_save(native_save_allowed);
    let capabilities = DocumentCapabilities {
        source_format,
        can_save_original: native_save_allowed,
        native_save_format: native_extension.clone(),
        export_formats,
        requires_save_as_for_native_save: native_extension.is_none(),
        native_save_extension: native_extension.clone(),
        export_extension,
        workbook: workbook.clone(),
    };
    let blocked_reason = native_save_blocked_reason(&capabilities);

    NativeSavePlan {
        can_save: blocked_reason.is_none(),
        requires_save_as: capabilities.requires_save_as_for_native_save,
        native_save_extension: native_extension.clone(),
        default_extension: native_extension.unwrap_or_else(default_extension_string),
        blocked_reason,
        capabilities,
    }
}

pub fn format_options() -> SpreadsheetFormatOptions {
    spreadsheet_format_options()
}

fn active_workbook_capabilities(
    file_name: &str,
    current_path: Option<&str>,
    native_save_allowed: bool,
) -> WorkbookCapabilities {
    let registry = active_document_store();
    let Ok(registry_guard) = registry.read() else {
        eprintln!("document registry lock poisoned while reading workbook capabilities");
        let mut capabilities = WorkbookCapabilities::default();
        capabilities.save.can_native_save = native_save_allowed;
        return capabilities;
    };
    registry_guard
        .active()
        .filter(|editor_state| {
            let active_file = editor_state.file_data();
            match current_path {
                Some(path) if !path.is_empty() => path == active_file.path,
                _ => active_file.file_name == file_name,
            }
        })
        .map(|editor_state| {
            let mut capabilities = editor_state.capabilities();
            capabilities.save.can_native_save =
                native_save_allowed && capabilities.save.can_native_save;
            capabilities
        })
        .unwrap_or_else(|| {
            let mut capabilities = WorkbookCapabilities::default();
            capabilities.save.can_native_save = native_save_allowed;
            capabilities
        })
}

fn active_workbook_capabilities_for_save(native_save_allowed: bool) -> WorkbookCapabilities {
    let registry = active_document_store();
    let Ok(registry_guard) = registry.read() else {
        eprintln!("document registry lock poisoned while planning native save");
        let mut capabilities = WorkbookCapabilities::default();
        capabilities.save.can_native_save = native_save_allowed;
        return capabilities;
    };

    registry_guard
        .active()
        .map(|editor_state| {
            let mut capabilities = editor_state.capabilities();
            capabilities.save.can_native_save =
                native_save_allowed && capabilities.save.can_native_save;
            capabilities
        })
        .unwrap_or_else(|| {
            let mut capabilities = WorkbookCapabilities::default();
            capabilities.save.can_native_save = native_save_allowed;
            capabilities
        })
}

fn native_save_blocked_reason(capabilities: &DocumentCapabilities) -> Option<String> {
    if capabilities.native_save_extension.is_none() {
        return Some("Native save is only supported as .xlsx or .csv.".to_string());
    }
    if !capabilities.workbook.save.can_native_save {
        return Some(first_reason(
            [
                &capabilities.workbook.save.blocked_save_reasons,
                &capabilities.workbook.structure.blocked_structure_reasons,
                &capabilities
                    .workbook
                    .structure
                    .blocked_sheet_structure_reasons,
                &capabilities.workbook.save.detected_features,
            ],
            "Workbook cannot be saved in its current state.",
        ));
    }
    None
}

fn first_reason<const N: usize>(reason_groups: [&Vec<String>; N], fallback: &str) -> String {
    reason_groups
        .into_iter()
        .flat_map(|reasons| reasons.iter())
        .next()
        .cloned()
        .unwrap_or_else(|| fallback.to_string())
}

fn init_editor_state(
    file_data: FileData,
    workbook: Option<Workbook>,
) -> Result<OpenDocumentResponse, AppError> {
    let registry = active_document_store();
    let initialized_file_data;
    let editor_session;
    let document_id;
    let previous_document_id;
    {
        let mut registry_guard = registry
            .write()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        previous_document_id = registry_guard
            .active()
            .map(|editor_state| editor_state.document_id());
        let editor_state = EditorState::with_workbook(file_data, workbook);
        initialized_file_data = editor_state.file_data().clone();
        editor_session = editor_session_info(&editor_state);
        document_id = editor_state.document_id();
        registry_guard.try_replace_active(editor_state)?;
    }
    if let Some(previous_document_id) = previous_document_id
        && previous_document_id != document_id
    {
        cancel_index_jobs_for_document(previous_document_id);
    }
    // 异步构建索引（后台线程）
    spawn_rebuild_all_sheets_index(&registry, document_id);
    Ok(OpenDocumentResponse {
        file_data: initialized_file_data,
        editor_session,
    })
}

fn editor_session_info(editor_state: &EditorState) -> EditorSessionInfo {
    EditorSessionInfo {
        document_id: editor_state.document_id(),
        revision: editor_state.revision(),
        formula_status: editor_state.formula_status(),
        capabilities: editor_state.capabilities(),
        editor_state: editor_state_info(editor_state),
    }
}

fn native_save_extension(file_name: &str) -> Option<String> {
    if extension_of(file_name).is_none() {
        Some(default_extension_string())
    } else {
        supported_extension_from_name(file_name)
    }
}

fn export_extension(file_name: &str) -> Option<String> {
    native_save_extension(file_name)
}

fn document_format(file_name: &str) -> Option<String> {
    export_extension(file_name)
}

fn export_formats_for(_source_format: &str) -> Vec<String> {
    export_extensions()
}

fn default_extension_string() -> String {
    default_spreadsheet_extension().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CellValue;

    #[test]
    fn document_capabilities_are_computed_by_backend() {
        assert_eq!(
            document_capabilities("book.xlsx", None),
            DocumentCapabilities {
                source_format: "xlsx".to_string(),
                can_save_original: true,
                native_save_format: Some("xlsx".to_string()),
                export_formats: vec!["xlsx".to_string(), "csv".to_string()],
                native_save_extension: Some("xlsx".to_string()),
                export_extension: "xlsx".to_string(),
                requires_save_as_for_native_save: false,
                workbook: WorkbookCapabilities::default(),
            }
        );
        assert_eq!(
            document_capabilities("data.csv", Some("/tmp/data.csv")),
            DocumentCapabilities {
                source_format: "csv".to_string(),
                can_save_original: true,
                native_save_format: Some("csv".to_string()),
                export_formats: vec!["xlsx".to_string(), "csv".to_string()],
                native_save_extension: Some("csv".to_string()),
                export_extension: "csv".to_string(),
                requires_save_as_for_native_save: false,
                workbook: WorkbookCapabilities::default(),
            }
        );
    }

    #[test]
    fn open_from_bytes_detects_extensionless_csv_content() {
        let response = open_from_bytes(
            "/tmp/imported".to_string(),
            b"name,score\nalice,42".to_vec(),
            Some("imported".to_string()),
        )
        .expect("open extensionless csv");

        let rows = &response.file_data.sheets[0].rows;
        assert_eq!(rows[0][0], CellValue::String("name".to_string()));
        assert_eq!(rows[0][1], CellValue::String("score".to_string()));
        assert_eq!(rows[1][0], CellValue::String("alice".to_string()));
        assert_eq!(rows[1][1], CellValue::Number(42.into()));
    }
}
