use std::path::Path;
use std::sync::Arc;
use std::sync::RwLock;

use crate::editor_state::EditorState;
use crate::error::AppError;
use crate::reader;
use crate::types::FileData;
use crate::writer;

/// 异步构建索引（后台线程）
fn spawn_index_build(_file_data: FileData, state: Arc<RwLock<Option<EditorState>>>) {
    std::thread::spawn(move || {
        if let Ok(mut guard) = state.write() {
            if let Some(ref mut editor_state) = *guard {
                for sheet in &mut editor_state.file_data.sheets {
                    crate::editor_state::rebuild_sheet_index(sheet);
                }
            }
        }
    });
}

/// 读取文件
pub fn do_read_file(path: String) -> Result<FileData, AppError> {
    let path = Path::new(&path);
    let file_data = reader::read_file(path)?;

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
        let mut state_guard = state.write().unwrap();
        *state_guard = Some(EditorState::new(file_data.clone()));
    }
    // 异步构建索引（后台线程）
    spawn_index_build(file_data.clone(), state.clone());
}

/// 保存文件
pub fn do_save_file(path: String, file_data: FileData) -> Result<(), AppError> {
    let path = Path::new(&path);
    writer::save_file(path, &file_data)?;

    // 更新编辑器状态中的文件数据
    let state = crate::commands::get_state();
    let mut state_guard = state.write().unwrap();
    if let Some(editor_state) = state_guard.as_mut() {
        editor_state.file_data = file_data;
    }

    Ok(())
}

/// 获取默认保存路径
pub fn do_get_default_save_path(file_name: String) -> String {
    if let Some(dot_pos) = file_name.rfind('.') {
        let name = &file_name[..dot_pos];
        format!("{}_edited.xlsx", name)
    } else {
        format!("{}_edited.xlsx", file_name)
    }
}
