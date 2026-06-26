use std::path::Path;

use crate::error::AppError;
use crate::io::codec::{reader::read_file_with_workbook_from_bytes, writer};
use crate::ops::index_ops::spawn_rebuild_all_sheets_index;
use crate::state::{editor_state::EditorState, get_state};
use crate::types::FileData;
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
    fallback_file_data: &FileData,
) -> Result<(String, Vec<u8>), AppError> {
    let state = get_state();
    let mut state_guard = state.write().expect("Editor state lock poisoned");

    if let Some(editor_state) = state_guard.as_mut() {
        editor_state.sync_layout_from_frontend(fallback_file_data);
        return editor_state.generate_file_bytes_for_target(target_path_or_name);
    }

    writer::generate_file_bytes_for_target(fallback_file_data, target_path_or_name)
}

fn init_editor_state(file_data: FileData, workbook: Option<Workbook>) -> FileData {
    let state = get_state();
    let initialized_file_data;
    {
        let mut state_guard = state.write().expect("Editor state lock poisoned");
        let editor_state = EditorState::with_workbook(file_data, workbook);
        initialized_file_data = editor_state.file_data().clone();
        *state_guard = Some(editor_state);
    }
    // 异步构建索引（后台线程）
    spawn_rebuild_all_sheets_index(state);
    initialized_file_data
}
