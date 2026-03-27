use crate::error::AppError;
use crate::ops::index_ops::spawn_rebuild_all_sheets_index;
use crate::state::editor_state::EditorState;
use crate::types::FileData;

/// 从字节读取文件（用于 Android content:// URI 场景）
pub fn do_read_file_bytes(path: String, bytes: Vec<u8>) -> Result<FileData, AppError> {
    let path_obj = std::path::Path::new(&path);
    let extension = path_obj
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_else(|| "xlsx".to_string());

    let file_name = path_obj
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let file_data = super::reader::read_file_from_bytes(&extension, bytes, file_name)?;

    // 初始化编辑器状态
    init_editor_state(file_data.clone());

    Ok(file_data)
}

/// 初始化编辑器状态（用于新建文件）
pub fn do_init_file(file_data: FileData) -> Result<(), AppError> {
    init_editor_state(file_data);
    Ok(())
}

fn init_editor_state(file_data: FileData) {
    let state = crate::commands::get_state();
    {
        let mut state_guard = state.write().expect("Editor state lock poisoned");
        *state_guard = Some(EditorState::new(file_data));
    }
    // 异步构建索引（后台线程）
    spawn_rebuild_all_sheets_index(state.clone());
}

/// 生成文件字节（用于 Android content:// URI 场景）
pub fn do_generate_file_bytes(file_data: FileData) -> Result<(String, Vec<u8>), AppError> {
    super::writer::generate_file_bytes(&file_data)
}
