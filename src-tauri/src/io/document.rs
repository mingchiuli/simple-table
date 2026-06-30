use std::path::Path;

use crate::error::AppError;
use crate::io::codec::reader::read_file_with_workbook_from_bytes;
use crate::ops::index_ops::spawn_rebuild_all_sheets_index;
use crate::state::{active_document_store, editor_state::EditorState};
use crate::types::{DocumentCapabilities, FileData};
use umya_spreadsheet::Workbook;

/// 从已读取的文件字节打开文档，并初始化编辑器状态
pub fn open_from_bytes(
    path: String,
    bytes: Vec<u8>,
    file_name: Option<String>,
) -> Result<FileData, AppError> {
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
    let file_data = init_editor_state(result.file_data, result.workbook);

    Ok(file_data)
}

/// 初始化编辑器状态（用于新建文件）
pub fn init_file(file_data: FileData) -> Result<(), AppError> {
    init_editor_state(file_data, None);
    Ok(())
}

pub fn generate_current_file_bytes_for_target(
    target_path_or_name: &str,
) -> Result<(String, Vec<u8>), AppError> {
    let registry = active_document_store();
    let registry_guard = registry.read().expect("Document registry lock poisoned");

    if let Some(editor_state) = registry_guard.active() {
        return editor_state.generate_file_bytes_for_target(target_path_or_name);
    }

    Err(AppError::NoFileLoaded)
}

pub fn document_capabilities(
    file_name: String,
    current_path: Option<String>,
) -> DocumentCapabilities {
    let source_name = current_path.as_deref().unwrap_or(&file_name);
    let native_extension = native_save_extension(source_name);
    let export_extension = export_extension(&file_name).unwrap_or_else(|| "xlsx".to_string());

    DocumentCapabilities {
        requires_save_as_for_native_save: native_extension.is_none(),
        native_save_extension: native_extension,
        export_extension,
    }
}

fn init_editor_state(file_data: FileData, workbook: Option<Workbook>) -> FileData {
    let registry = active_document_store();
    let initialized_file_data;
    let document_id;
    {
        let mut registry_guard = registry.write().expect("Document registry lock poisoned");
        let editor_state = EditorState::with_workbook(file_data, workbook);
        initialized_file_data = editor_state.file_data().clone();
        document_id = editor_state.document_id();
        registry_guard.replace_active(editor_state);
    }
    // 异步构建索引（后台线程）
    spawn_rebuild_all_sheets_index(registry, document_id);
    initialized_file_data
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
            document_capabilities("book.xlsx".to_string(), None),
            DocumentCapabilities {
                native_save_extension: Some("xlsx".to_string()),
                export_extension: "xlsx".to_string(),
                requires_save_as_for_native_save: false,
            }
        );
        assert_eq!(
            document_capabilities("data.csv".to_string(), Some("/tmp/data.csv".to_string())),
            DocumentCapabilities {
                native_save_extension: None,
                export_extension: "csv".to_string(),
                requires_save_as_for_native_save: true,
            }
        );
    }
}
