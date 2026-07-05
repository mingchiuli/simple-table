use std::sync::{Arc, OnceLock, RwLock};

use crate::state::editor_state::EditorState;
use crate::types::{FormulaStatus, WorkbookCapabilities};
use ts_rs::TS;

/// 获取编辑器状态信息
#[derive(serde::Serialize, serde::Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct EditorStateInfo {
    pub can_undo: bool,
    pub can_redo: bool,
    pub is_dirty: bool,
}

#[derive(serde::Serialize, serde::Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct EditorSessionInfo {
    #[ts(type = "number")]
    pub document_id: u64,
    #[ts(type = "number")]
    pub revision: u64,
    pub formula_status: FormulaStatus,
    pub capabilities: WorkbookCapabilities,
    pub editor_state: EditorStateInfo,
}

pub struct ActiveDocumentStore {
    active: Option<EditorState>,
}

impl ActiveDocumentStore {
    fn new() -> Self {
        Self { active: None }
    }

    #[cfg(test)]
    pub fn new_for_test() -> Self {
        Self::new()
    }

    pub fn replace_active(&mut self, editor_state: EditorState) -> u64 {
        let document_id = editor_state.document_id();
        self.active = Some(editor_state);
        document_id
    }

    pub fn active(&self) -> Option<&EditorState> {
        self.active.as_ref()
    }

    pub fn active_mut(&mut self) -> Option<&mut EditorState> {
        self.active.as_mut()
    }

    pub fn get(&self, document_id: u64) -> Option<&EditorState> {
        self.active()
            .filter(|editor_state| editor_state.document_id() == document_id)
    }

    pub fn get_mut(&mut self, document_id: u64) -> Option<&mut EditorState> {
        self.active_mut()
            .filter(|editor_state| editor_state.document_id() == document_id)
    }
}

/// 全局活动文档状态。Simple Table 当前是单文档 UI；documentId 只用于丢弃过期异步任务。
static ACTIVE_DOCUMENT_STORE: OnceLock<Arc<RwLock<ActiveDocumentStore>>> = OnceLock::new();

pub fn active_document_store() -> Arc<RwLock<ActiveDocumentStore>> {
    ACTIVE_DOCUMENT_STORE
        .get_or_init(|| Arc::new(RwLock::new(ActiveDocumentStore::new())))
        .clone()
}
