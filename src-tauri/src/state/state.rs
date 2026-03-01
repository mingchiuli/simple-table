/// 获取编辑器状态信息
#[derive(serde::Serialize)]
pub struct EditorStateInfo {
    pub can_undo: bool,
    pub can_redo: bool,
}
