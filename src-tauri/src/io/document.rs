use std::path::Path;

use crate::error::AppError;
use crate::io::codec::reader::read_file_with_workbook_from_bytes;
use crate::ops::index_ops::spawn_rebuild_all_sheets_index;
use crate::ops::patch_projector::editor_state_info;
use crate::state::{active_document_store, editor_state::EditorState, state::EditorSessionInfo};
use crate::types::{DocumentCapabilities, FileData, OpenDocumentResponse, WorkbookCapabilities};
use umya_spreadsheet::Workbook;

/// 从已读取的文件字节打开文档，并初始化编辑器状态
pub fn open_from_bytes(
    path: String,
    bytes: Vec<u8>,
    file_name: Option<String>,
) -> Result<OpenDocumentResponse, AppError> {
    let path_obj = Path::new(&path);
    let extension = path_obj
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_else(|| "xlsx".to_string());

    // 如果调用方已经解析出文件名，优先使用；否则从路径解析
    let resolved_file_name = file_name.unwrap_or_else(|| {
        path_obj
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

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
    let document = {
        let registry_guard = registry
            .read()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        registry_guard
            .active()
            .map(|editor_state| editor_state.document_snapshot())
            .ok_or(AppError::NoFileLoaded)?
    };

    document.generate_file_bytes_for_target(target_path_or_name)
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

pub fn update_current_file_identity(path: String, file_name: String) -> Result<(), AppError> {
    let registry = active_document_store();
    let mut registry_guard = registry
        .write()
        .map_err(|_| AppError::poisoned_lock("document registry"))?;

    let editor_state = registry_guard.active_mut().ok_or(AppError::NoFileLoaded)?;
    editor_state.update_identity(path, file_name);
    Ok(())
}

pub fn document_capabilities(file_name: &str, current_path: Option<&str>) -> DocumentCapabilities {
    let source_name = current_path.unwrap_or(file_name);
    let native_extension = native_save_extension(source_name);
    let native_save_allowed = native_extension.is_some();
    let export_extension = export_extension(file_name).unwrap_or_else(|| "xlsx".to_string());

    DocumentCapabilities {
        requires_save_as_for_native_save: native_extension.is_none(),
        native_save_extension: native_extension,
        export_extension,
        workbook: active_workbook_capabilities(file_name, current_path, native_save_allowed),
    }
}

fn active_workbook_capabilities(
    file_name: &str,
    current_path: Option<&str>,
    native_save_allowed: bool,
) -> WorkbookCapabilities {
    let registry = active_document_store();
    let Ok(registry_guard) = registry.read() else {
        eprintln!("document registry lock poisoned while reading workbook capabilities");
        return WorkbookCapabilities {
            can_native_save: native_save_allowed,
            ..Default::default()
        };
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
            capabilities.can_native_save = native_save_allowed && capabilities.can_native_save;
            capabilities
        })
        .unwrap_or_else(|| WorkbookCapabilities {
            can_native_save: native_save_allowed,
            ..Default::default()
        })
}

fn init_editor_state(
    file_data: FileData,
    workbook: Option<Workbook>,
) -> Result<OpenDocumentResponse, AppError> {
    let registry = active_document_store();
    let initialized_file_data;
    let editor_session;
    let document_id;
    {
        let mut registry_guard = registry
            .write()
            .map_err(|_| AppError::poisoned_lock("document registry"))?;
        let editor_state = EditorState::with_workbook(file_data, workbook);
        initialized_file_data = editor_state.file_data().clone();
        editor_session = EditorSessionInfo {
            document_id: editor_state.document_id(),
            revision: editor_state.revision(),
            formula_status: editor_state.formula_status(),
            capabilities: editor_state.capabilities(),
            editor_state: editor_state_info(&editor_state),
        };
        document_id = editor_state.document_id();
        registry_guard.replace_active(editor_state);
    }
    // 异步构建索引（后台线程）
    spawn_rebuild_all_sheets_index(&registry, document_id);
    Ok(OpenDocumentResponse {
        file_data: initialized_file_data,
        editor_session,
    })
}

fn native_save_extension(file_name: &str) -> Option<String> {
    let extension = extension_of(file_name).unwrap_or_else(|| "xlsx".to_string());
    (extension == "xlsx").then_some(extension)
}

fn export_extension(file_name: &str) -> Option<String> {
    let extension = extension_of(file_name).unwrap_or_else(|| "xlsx".to_string());
    matches!(extension.as_str(), "xlsx" | "csv").then_some(extension)
}

fn extension_of(file_name: &str) -> Option<String> {
    Path::new(file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| !extension.is_empty())
        .map(|extension| extension.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_capabilities_are_computed_by_backend() {
        assert_eq!(
            document_capabilities("book.xlsx", None),
            DocumentCapabilities {
                native_save_extension: Some("xlsx".to_string()),
                export_extension: "xlsx".to_string(),
                requires_save_as_for_native_save: false,
                workbook: WorkbookCapabilities::default(),
            }
        );
        assert_eq!(
            document_capabilities("data.csv", Some("/tmp/data.csv")),
            DocumentCapabilities {
                native_save_extension: None,
                export_extension: "csv".to_string(),
                requires_save_as_for_native_save: true,
                workbook: WorkbookCapabilities {
                    can_native_save: false,
                    ..Default::default()
                },
            }
        );
    }
}
