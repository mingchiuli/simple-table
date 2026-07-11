use crate::error::AppError;
use crate::io::codec::reader::read_file_with_workbook_from_bytes;
use crate::io::file_format::{
    default_spreadsheet_extension, export_extensions, extension_of, file_name_from_path_like,
    is_xlsx_extension, open_extension_from_path_name_or_bytes, spreadsheet_format_options,
    supported_extension_from_name,
};
use crate::io::prepared_documents;
use crate::io::projection_limits::validate_file_data;
use crate::ops::index_ops::{cancel_index_jobs_for_document, spawn_rebuild_all_sheets_index};
use crate::ops::patch_projector::editor_state_info;
use crate::state::{
    active_document_store,
    editor_state::{EditorState, SaveCommitLease},
    state::EditorSessionInfo,
};
use crate::types::{
    DocumentCapabilities, FileData, NativeSavePlan, OpenDocumentResponse, PreparedOpenDocument,
    SavedDocumentIdentity, SavedDocumentResponse, SheetData, SheetProjectionResponse, SheetRegion,
    SheetRegionProjectionResponse, SpreadsheetFormatOptions, WorkbookCapabilities,
};
use std::path::PathBuf;
use umya_spreadsheet::Workbook;

const LOSSY_CSV_SAVE_REASON: &str = "Saving a non-CSV document as CSV would discard sheets, formulas, or formatting; use Export instead.";
const INITIAL_REGION_ROWS: usize = 256;
const INITIAL_REGION_COLUMNS: usize = 128;
const MAX_REGION_CELLS: usize = 65_536;
const MAX_REGION_ROWS: usize = 1_024;
const MAX_REGION_COLUMNS: usize = 512;

/// 从已读取的文件字节打开文档，并初始化编辑器状态
pub fn prepare_open_from_bytes(
    path: String,
    bytes: Vec<u8>,
    file_name: Option<String>,
) -> Result<PreparedOpenDocument, AppError> {
    let extension = open_extension_from_path_name_or_bytes(&path, file_name.as_deref(), &bytes);

    // 如果调用方已经解析出文件名，优先使用；否则从路径解析
    let resolved_file_name =
        file_name.unwrap_or_else(|| file_name_from_path_like(&path, "unknown"));

    // 传入 path 到 reader，同时保留 Excel 原始 Workbook 用于后续无损 patch 保存。
    let source_path = PathBuf::from(&path);
    let result = read_file_with_workbook_from_bytes(&extension, bytes, path, resolved_file_name)?;

    prepare_editor_state(result.file_data, result.workbook, Some(source_path))
}

/// 准备新文档。只有 commit_prepared_document 才会替换当前活动文档。
pub fn prepare_new_file(mut file_data: FileData) -> Result<PreparedOpenDocument, AppError> {
    file_data.path.clear();
    validate_file_data(&file_data)?;
    prepare_editor_state(file_data, None, None)
}

pub fn commit_prepared_document(
    token: &str,
    expected_document_id: Option<u64>,
    expected_revision: Option<u64>,
) -> Result<OpenDocumentResponse, AppError> {
    let registry = active_document_store();
    let previous_document_id;
    let document_id;
    let source_path;
    let response;
    {
        let mut registry_guard = registry
            .write()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        (document_id, previous_document_id, source_path) = registry_guard
            .replace_active_for_context(expected_document_id, expected_revision, || {
                let prepared = prepared_documents::take(token)?;
                Ok((prepared.editor_state, prepared.source_path))
            })?;
        let editor_state = registry_guard.active().ok_or(AppError::NoFileLoaded)?;
        response = open_document_response(editor_state);
    }

    if let Some(previous_document_id) = previous_document_id
        && previous_document_id != document_id
    {
        cancel_index_jobs_for_document(previous_document_id);
    }
    adopt_source_path_if_transient(source_path.as_deref());
    spawn_rebuild_all_sheets_index(&registry, document_id);
    Ok(response)
}

pub fn abort_prepared_document(token: &str) -> Result<(), AppError> {
    prepared_documents::abort(token)
}

/// Restores the frontend after its runtime state was lost while the Rust process stayed alive.
pub fn active_document_response() -> Result<Option<OpenDocumentResponse>, AppError> {
    let registry = active_document_store();
    let registry_guard = registry
        .read()
        .map_err(|_| AppError::poisoned_lock("document registry"))?;
    Ok(registry_guard.active().map(open_document_response))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub(crate) fn active_document_path() -> Result<Option<String>, AppError> {
    let registry = active_document_store();
    let registry_guard = registry
        .read()
        .map_err(|_| AppError::poisoned_lock("document registry"))?;
    Ok(registry_guard
        .active()
        .map(|editor_state| editor_state.file_data().path.clone())
        .filter(|path| !path.is_empty()))
}

pub fn generate_current_file_bytes_for_target(
    document_id: u64,
    base_revision: u64,
    target_path_or_name: &str,
) -> Result<(String, Vec<u8>), AppError> {
    let registry = active_document_store();
    let snapshot = {
        let registry_guard = registry
            .read()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        registry_guard
            .active_for_command(document_id, base_revision)?
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
    document_id_token: u64,
    revision_token: u64,
    target_path_or_name: &str,
) -> Result<PreparedDocumentSave, AppError> {
    let registry = active_document_store();
    let snapshot;
    {
        let registry_guard = registry
            .read()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        let editor_state = registry_guard.active_for_command(document_id_token, revision_token)?;
        ensure_native_save_target_allowed(editor_state, target_path_or_name)?;
        if editor_state.has_save_commit_in_progress() {
            return Err(AppError::DocumentStateInvalid(
                "save is already in progress".to_string(),
            ));
        }
        snapshot = editor_state.save_snapshot_for_target(target_path_or_name)?;
    }

    let (output_name, bytes) = snapshot.generate_file_bytes_for_target(target_path_or_name)?;
    let target_extension = extension_of(&output_name)
        .or_else(|| extension_of(target_path_or_name))
        .unwrap_or_else(default_extension_string);
    Ok(PreparedDocumentSave {
        document_id: document_id_token,
        revision: revision_token,
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
    let result = read_file_with_workbook_from_bytes(&extension, bytes, path.clone(), output_name)?;
    let registry = active_document_store();
    let (lease, clear_history) = begin_prepared_save_commit(
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
    )?;

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
            file_data: Some(editor_state.file_data().clone()),
            identity: None,
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
    let (lease, clear_history) = begin_prepared_save_commit(
        &registry,
        document_id_token,
        revision_token,
        |editor_state| {
            let current_extension = extension_of(&editor_state.file_data().file_name)
                .or_else(|| extension_of(&editor_state.file_data().path));
            current_extension.as_deref() != Some(saved_extension.as_str())
        },
    )?;

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
            file_data: None,
            identity: Some(SavedDocumentIdentity {
                path: editor_state.file_data().path.clone(),
                file_name: editor_state.file_data().file_name.clone(),
            }),
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

pub fn current_file_data_for_command(
    document_id: u64,
    base_revision: u64,
) -> Result<FileData, AppError> {
    inspect_current_file_for_command(document_id, base_revision, Clone::clone)
}

pub fn sheet_projection_for_command(
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
) -> Result<SheetProjectionResponse, AppError> {
    let registry = active_document_store();
    let registry_guard = registry
        .read()
        .map_err(|_| AppError::poisoned_lock("document registry"))?;
    let editor_state = registry_guard.active_for_command(document_id, base_revision)?;
    let source_sheet = editor_state
        .file_data()
        .sheets
        .get(sheet_index)
        .ok_or(AppError::InvalidSheetIndex(sheet_index))?;
    let extent = editor_state
        .sheet_extent(sheet_index)
        .ok_or(AppError::InvalidSheetIndex(sheet_index))?;
    let loaded_region = initial_sheet_region(sheet_index, &extent);
    let sheet = project_sheet_with_region(source_sheet, &loaded_region);
    Ok(SheetProjectionResponse {
        document_id,
        revision: base_revision,
        sheet_index,
        sheet,
        extent,
        loaded_region,
    })
}

pub fn sheet_region_projection_for_command(
    document_id: u64,
    base_revision: u64,
    region: SheetRegion,
) -> Result<SheetRegionProjectionResponse, AppError> {
    validate_sheet_region(&region)?;
    let registry = active_document_store();
    let registry_guard = registry
        .read()
        .map_err(|_| AppError::poisoned_lock("document registry"))?;
    let editor_state = registry_guard.active_for_command(document_id, base_revision)?;
    let sheet = editor_state
        .file_data()
        .sheets
        .get(region.sheet_index)
        .ok_or(AppError::InvalidSheetIndex(region.sheet_index))?;
    let extent = editor_state
        .sheet_extent(region.sheet_index)
        .ok_or(AppError::InvalidSheetIndex(region.sheet_index))?;
    if region.row_end > extent.row_count || region.col_end > extent.column_count {
        return Err(AppError::DocumentStateInvalid(
            "sheet region exceeds the current sheet extent".to_string(),
        ));
    }
    let cells = project_region_cells(sheet, &region);
    Ok(SheetRegionProjectionResponse {
        document_id,
        revision: base_revision,
        region,
        cells,
    })
}

pub(crate) fn inspect_current_file_for_command<T>(
    document_id: u64,
    base_revision: u64,
    inspect: impl FnOnce(&FileData) -> T,
) -> Result<T, AppError> {
    let registry = active_document_store();
    let registry_guard = registry
        .read()
        .map_err(|_| AppError::poisoned_lock("document registry"))?;

    registry_guard
        .active_for_command(document_id, base_revision)
        .map(|editor_state| inspect(editor_state.file_data()))
}

pub fn close_current_document(document_id: u64) -> Result<(), AppError> {
    let registry = active_document_store();
    let closed_document_id = {
        let mut registry_guard = registry
            .write()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        registry_guard.close_active_document(document_id)?
    };
    if let Some(document_id) = closed_document_id {
        cancel_index_jobs_for_document(document_id);
    }
    Ok(())
}

#[cfg(test)]
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

pub fn document_capabilities_for_command(
    document_id: u64,
    base_revision: u64,
) -> Result<DocumentCapabilities, AppError> {
    let (file_name, current_path) =
        inspect_current_file_for_command(document_id, base_revision, |file_data| {
            (
                file_data.file_name.clone(),
                (!file_data.path.is_empty()).then(|| file_data.path.clone()),
            )
        })?;
    let current_path = current_path.as_deref();
    let file_name = file_name.as_str();
    let source_name = current_path.unwrap_or(file_name);
    let source_format = document_format(source_name)
        .or_else(|| document_format(file_name))
        .unwrap_or_else(default_extension_string);
    let native_extension = native_save_extension(source_name);
    let native_save_allowed = native_extension.is_some();
    let export_extension = export_extension(file_name).unwrap_or_else(|| source_format.clone());
    let export_formats = export_formats_for(&source_format);
    let workbook =
        workbook_capabilities_for_command(document_id, base_revision, native_save_allowed)?;

    Ok(DocumentCapabilities {
        source_format,
        can_save_original: native_save_allowed,
        native_save_format: native_extension.clone(),
        export_formats,
        requires_save_as_for_native_save: native_extension.is_none(),
        native_save_extension: native_extension,
        export_extension,
        workbook,
    })
}

pub fn native_save_plan_for_command(
    document_id: u64,
    base_revision: u64,
    target_path_or_name: &str,
) -> Result<NativeSavePlan, AppError> {
    let source_format =
        document_format(target_path_or_name).unwrap_or_else(default_extension_string);
    let native_extension = native_save_extension(target_path_or_name);
    let native_save_allowed = native_extension.is_some();
    let export_extension =
        export_extension(target_path_or_name).unwrap_or_else(|| source_format.clone());
    let export_formats = export_formats_for(&source_format);
    let workbook = native_save_workbook_capabilities_for_command(
        document_id,
        base_revision,
        native_save_allowed,
        target_path_or_name,
    )?;
    let capabilities = DocumentCapabilities {
        source_format,
        can_save_original: native_save_allowed,
        native_save_format: native_extension.clone(),
        export_formats,
        requires_save_as_for_native_save: native_extension.is_none(),
        native_save_extension: native_extension.clone(),
        export_extension,
        workbook,
    };
    let blocked_reason = native_save_blocked_reason(&capabilities);

    Ok(NativeSavePlan {
        can_save: blocked_reason.is_none(),
        requires_save_as: capabilities.requires_save_as_for_native_save,
        native_save_extension: native_extension.clone(),
        default_extension: native_extension.unwrap_or_else(default_extension_string),
        blocked_reason,
        capabilities,
    })
}

pub fn format_options() -> SpreadsheetFormatOptions {
    spreadsheet_format_options()
}

#[cfg(test)]
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

fn workbook_capabilities_for_command(
    document_id: u64,
    base_revision: u64,
    native_save_allowed: bool,
) -> Result<WorkbookCapabilities, AppError> {
    workbook_capabilities_for_command_and_target(
        document_id,
        base_revision,
        native_save_allowed,
        None,
    )
}

fn native_save_workbook_capabilities_for_command(
    document_id: u64,
    base_revision: u64,
    native_save_allowed: bool,
    target_path_or_name: &str,
) -> Result<WorkbookCapabilities, AppError> {
    workbook_capabilities_for_command_and_target(
        document_id,
        base_revision,
        native_save_allowed,
        Some(target_path_or_name),
    )
}

fn workbook_capabilities_for_command_and_target(
    document_id: u64,
    base_revision: u64,
    native_save_allowed: bool,
    target_path_or_name: Option<&str>,
) -> Result<WorkbookCapabilities, AppError> {
    let registry = active_document_store();
    let registry_guard = registry
        .read()
        .map_err(|_| AppError::poisoned_lock("document registry"))?;
    let editor_state = registry_guard.active_for_command(document_id, base_revision)?;
    let mut capabilities = editor_state.capabilities();
    capabilities.save.can_native_save = native_save_allowed && capabilities.save.can_native_save;
    if let Some(reason) =
        target_path_or_name.and_then(|target| native_save_target_block_reason(editor_state, target))
    {
        capabilities.save.can_native_save = false;
        if !capabilities
            .save
            .blocked_save_reasons
            .iter()
            .any(|item| item == reason)
        {
            capabilities
                .save
                .blocked_save_reasons
                .push(reason.to_string());
        }
    }
    Ok(capabilities)
}

fn ensure_native_save_target_allowed(
    editor_state: &EditorState,
    target_path_or_name: &str,
) -> Result<(), AppError> {
    if let Some(reason) = native_save_target_block_reason(editor_state, target_path_or_name) {
        return Err(AppError::DocumentStateInvalid(reason.to_string()));
    }
    Ok(())
}

fn native_save_target_block_reason(
    editor_state: &EditorState,
    target_path_or_name: &str,
) -> Option<&'static str> {
    let target_extension =
        extension_of(target_path_or_name).unwrap_or_else(default_extension_string);
    (target_extension == "csv" && !editor_state.is_csv_backed()).then_some(LOSSY_CSV_SAVE_REASON)
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

fn prepare_editor_state(
    file_data: FileData,
    workbook: Option<Workbook>,
    source_path: Option<PathBuf>,
) -> Result<PreparedOpenDocument, AppError> {
    let editor_state = EditorState::with_workbook(file_data, workbook);
    let token = prepared_documents::replace(editor_state, source_path)?;
    Ok(PreparedOpenDocument { token })
}

fn adopt_source_path_if_transient(source_path: Option<&std::path::Path>) {
    #[cfg(any(target_os = "android", target_os = "ios", test))]
    if let Some(source_path) = source_path {
        let _ =
            crate::io::transient_files::transient_file_registry().adopt_if_registered(source_path);
    }

    #[cfg(not(any(target_os = "android", target_os = "ios", test)))]
    let _ = source_path;
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

fn open_document_response(editor_state: &EditorState) -> OpenDocumentResponse {
    let source = editor_state.file_data();
    let sheet_extents = editor_state.sheet_extents();
    let loaded_sheet_indexes = (!source.sheets.is_empty())
        .then_some(0)
        .into_iter()
        .collect();
    let loaded_sheet_regions = (!source.sheets.is_empty())
        .then(|| initial_sheet_region(0, &sheet_extents[0]))
        .into_iter()
        .collect::<Vec<_>>();
    let sheets = source
        .sheets
        .iter()
        .enumerate()
        .map(|(sheet_index, sheet)| {
            if sheet_index == 0 {
                project_sheet_with_region(sheet, &loaded_sheet_regions[0])
            } else {
                SheetData {
                    name: sheet.name.clone(),
                    ..Default::default()
                }
            }
        })
        .collect();
    OpenDocumentResponse {
        file_data: FileData {
            path: source.path.clone(),
            file_name: source.file_name.clone(),
            sheets,
        },
        editor_session: editor_session_info(editor_state),
        sheet_extents: Some(sheet_extents),
        loaded_sheet_indexes: Some(loaded_sheet_indexes),
        loaded_sheet_regions: Some(loaded_sheet_regions),
    }
}

fn initial_sheet_region(sheet_index: usize, extent: &crate::types::SheetExtent) -> SheetRegion {
    SheetRegion {
        sheet_index,
        row_start: 0,
        row_end: extent.row_count.min(INITIAL_REGION_ROWS),
        col_start: 0,
        col_end: extent.column_count.min(INITIAL_REGION_COLUMNS),
    }
}

fn validate_sheet_region(region: &SheetRegion) -> Result<(), AppError> {
    if region.row_start > region.row_end || region.col_start > region.col_end {
        return Err(AppError::DocumentStateInvalid(
            "invalid sheet region bounds".to_string(),
        ));
    }
    let cells = region
        .row_end
        .saturating_sub(region.row_start)
        .saturating_mul(region.col_end.saturating_sub(region.col_start));
    let row_count = region.row_end.saturating_sub(region.row_start);
    let column_count = region.col_end.saturating_sub(region.col_start);
    if row_count > MAX_REGION_ROWS || column_count > MAX_REGION_COLUMNS {
        return Err(AppError::ResourceLimitExceeded(format!(
            "sheet region dimensions are {row_count}x{column_count}, maximum is {MAX_REGION_ROWS}x{MAX_REGION_COLUMNS}"
        )));
    }
    if cells > MAX_REGION_CELLS {
        return Err(AppError::ResourceLimitExceeded(format!(
            "sheet region contains {cells} cells, maximum is {MAX_REGION_CELLS}"
        )));
    }
    if region.row_end > crate::io::projection_limits::MAX_ROWS_PER_SHEET
        || region.col_end > crate::io::projection_limits::MAX_COLUMNS_PER_ROW
    {
        return Err(AppError::ResourceLimitExceeded(
            "sheet region exceeds row or column limits".to_string(),
        ));
    }
    Ok(())
}

fn project_sheet_with_region(sheet: &SheetData, region: &SheetRegion) -> SheetData {
    let mut projected = sheet.clone();
    projected.rows = project_region_rows(sheet, region);
    projected
}

fn project_region_rows(
    sheet: &SheetData,
    region: &SheetRegion,
) -> Vec<Vec<crate::types::CellValue>> {
    (region.row_start..region.row_end)
        .map(|row_index| {
            let row = sheet
                .rows
                .get(row_index)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let start = region.col_start.min(row.len());
            let end = region.col_end.min(row.len());
            row[start..end].to_vec()
        })
        .collect()
}

fn project_region_cells(
    sheet: &SheetData,
    region: &SheetRegion,
) -> Vec<crate::types::SheetCellChange> {
    let mut cells = Vec::new();
    for row_index in region.row_start..region.row_end {
        let Some(row) = sheet.rows.get(row_index) else {
            continue;
        };
        for (col_index, value) in row
            .iter()
            .enumerate()
            .take(region.col_end.min(row.len()))
            .skip(region.col_start)
        {
            let value = value.clone();
            cells.push(
                crate::types::SheetCellChange::new(region.sheet_index, row_index, col_index, value)
                    .with_display_projection(
                        sheet.cell_display_text(row_index, col_index),
                        sheet.cell_format_at(row_index, col_index),
                        sheet.cell_style_at(row_index, col_index),
                    ),
            );
        }
    }
    cells
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
    use crate::types::{CellValue, SheetData};

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
        let prepared = prepare_open_from_bytes(
            "/tmp/imported".to_string(),
            b"name,score\nalice,42".to_vec(),
            Some("imported".to_string()),
        )
        .expect("open extensionless csv");
        let response = prepared_documents::take(&prepared.token).expect("prepared document");

        let rows = &response.editor_state.file_data().sheets[0].rows;
        assert_eq!(rows[0][0], CellValue::String("name".to_string()));
        assert_eq!(rows[0][1], CellValue::String("score".to_string()));
        assert_eq!(rows[1][0], CellValue::String("alice".to_string()));
        assert_eq!(rows[1][1], CellValue::Number(42.into()));
    }

    #[test]
    fn init_file_does_not_trust_frontend_path() {
        let prepared = prepare_new_file(FileData {
            path: "/tmp/should-not-be-trusted.xlsx".to_string(),
            file_name: "untitled.xlsx".to_string(),
            sheets: vec![SheetData::default()],
        })
        .expect("init file");
        let response = prepared_documents::take(&prepared.token).expect("prepared document");

        assert_eq!(response.editor_state.file_data().path, "");
        assert_eq!(response.editor_state.file_data().file_name, "untitled.xlsx");
    }

    #[test]
    fn open_document_response_only_projects_the_initial_sheet() {
        let first_sheet = SheetData {
            name: "First".to_string(),
            rows: vec![vec![CellValue::String("loaded".to_string())]],
            ..Default::default()
        };
        let second_sheet = SheetData {
            name: "Second".to_string(),
            rows: vec![vec![CellValue::String("deferred".to_string())]],
            ..Default::default()
        };
        let state = EditorState::with_workbook(
            FileData {
                path: "/tmp/book.xlsx".to_string(),
                file_name: "book.xlsx".to_string(),
                sheets: vec![first_sheet, second_sheet],
            },
            None,
        );

        let response = open_document_response(&state);

        assert_eq!(response.file_data.sheets[0].rows.len(), 1);
        assert!(response.file_data.sheets[1].rows.is_empty());
        assert_eq!(response.file_data.sheets[1].name, "Second");
        assert_eq!(response.loaded_sheet_indexes, Some(vec![0]));
        assert_eq!(
            response.sheet_extents,
            Some(vec![
                crate::types::SheetExtent {
                    row_count: 1,
                    column_count: 1,
                },
                crate::types::SheetExtent {
                    row_count: 1,
                    column_count: 1,
                },
            ])
        );
    }

    #[test]
    fn region_projection_keeps_absolute_cell_coordinates() {
        let sheet = SheetData {
            rows: vec![
                vec![
                    CellValue::String("A1".into()),
                    CellValue::String("B1".into()),
                ],
                vec![
                    CellValue::String("A2".into()),
                    CellValue::String("B2".into()),
                ],
            ],
            ..Default::default()
        };
        let region = SheetRegion {
            sheet_index: 0,
            row_start: 1,
            row_end: 2,
            col_start: 1,
            col_end: 2,
        };

        let cells = project_region_cells(&sheet, &region);

        assert_eq!(cells.len(), 1);
        assert_eq!((cells[0].row, cells[0].col), (1, 1));
        assert_eq!(cells[0].display.as_deref(), Some("B2"));
    }

    #[test]
    fn region_projection_rejects_degenerate_oversized_dimensions() {
        let region = SheetRegion {
            sheet_index: 0,
            row_start: 0,
            row_end: MAX_REGION_ROWS + 1,
            col_start: 0,
            col_end: 0,
        };

        assert!(matches!(
            validate_sheet_region(&region),
            Err(AppError::ResourceLimitExceeded(_))
        ));
    }

    #[test]
    fn native_save_rejects_lossy_csv_conversion_but_export_remains_available() {
        let state = EditorState::with_workbook(
            FileData {
                path: String::new(),
                file_name: "untitled.xlsx".to_string(),
                sheets: vec![SheetData::default(), SheetData::default()],
            },
            None,
        );

        let error = ensure_native_save_target_allowed(&state, "converted.csv")
            .expect_err("native save must reject lossy CSV conversion");

        assert!(error.to_string().contains("use Export instead"));
        assert!(state.generate_file_bytes_for_target("export.csv").is_ok());
    }

    #[test]
    fn native_save_allows_an_existing_csv_document_to_remain_csv() {
        let state = EditorState::with_workbook(
            FileData {
                path: "/tmp/data.csv".to_string(),
                file_name: "data.csv".to_string(),
                sheets: vec![SheetData::default()],
            },
            None,
        );

        assert!(ensure_native_save_target_allowed(&state, "/tmp/data.csv").is_ok());
    }
}
