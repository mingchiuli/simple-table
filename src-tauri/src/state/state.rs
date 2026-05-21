use std::sync::{Arc, OnceLock, RwLock};

use crate::state::editor_state::EditorState;

/// 获取编辑器状态信息
#[derive(serde::Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EditorStateInfo {
    pub can_undo: bool,
    pub can_redo: bool,
}

/// 全局编辑器状态（使用 Arc<RwLock> 支持多线程访问）
static EDITOR_STATE: OnceLock<Arc<RwLock<Option<EditorState>>>> = OnceLock::new();

pub fn get_state() -> Arc<RwLock<Option<EditorState>>> {
    EDITOR_STATE
        .get_or_init(|| Arc::new(RwLock::new(None)))
        .clone()
}
