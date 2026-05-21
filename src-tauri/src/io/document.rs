use std::path::Path;

use crate::error::AppError;
use crate::ops::index_ops::spawn_rebuild_all_sheets_index;
use crate::state::{editor_state::EditorState, get_state};
use crate::types::FileData;

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

    // 传入 path 到 reader
    let file_data = super::codec::reader::read_file_from_bytes(
        &extension,
        bytes,
        path.clone(),
        resolved_file_name,
    )?;

    // 初始化编辑器状态
    init_editor_state(file_data.clone());

    Ok(file_data)
}

/// 初始化编辑器状态（用于新建文件）
pub fn init_file(file_data: FileData) -> Result<(), AppError> {
    init_editor_state(file_data);
    Ok(())
}

fn init_editor_state(file_data: FileData) {
    let state = get_state();
    {
        let mut state_guard = state.write().expect("Editor state lock poisoned");
        *state_guard = Some(EditorState::new(file_data));
    }
    // 异步构建索引（后台线程）
    spawn_rebuild_all_sheets_index(state.clone());
}
