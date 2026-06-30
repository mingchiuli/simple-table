use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use crate::state::editor_state::EditorState;
use crate::types::FormulaStatus;

/// 获取编辑器状态信息
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EditorStateInfo {
    pub can_undo: bool,
    pub can_redo: bool,
    pub is_dirty: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EditorSessionInfo {
    pub document_id: u64,
    pub revision: u64,
    pub formula_status: FormulaStatus,
    pub editor_state: EditorStateInfo,
}

pub struct DocumentRegistry {
    active_document_id: Option<u64>,
    documents: HashMap<u64, EditorState>,
}

impl DocumentRegistry {
    fn new() -> Self {
        Self {
            active_document_id: None,
            documents: HashMap::new(),
        }
    }

    #[cfg(test)]
    pub fn new_for_test() -> Self {
        Self::new()
    }

    pub fn replace_active(&mut self, editor_state: EditorState) -> u64 {
        let document_id = editor_state.document_id();
        self.documents.clear();
        self.documents.insert(document_id, editor_state);
        self.active_document_id = Some(document_id);
        document_id
    }

    pub fn active(&self) -> Option<&EditorState> {
        self.active_document_id
            .and_then(|document_id| self.documents.get(&document_id))
    }

    pub fn active_mut(&mut self) -> Option<&mut EditorState> {
        let document_id = self.active_document_id?;
        self.documents.get_mut(&document_id)
    }

    pub fn get(&self, document_id: u64) -> Option<&EditorState> {
        self.documents.get(&document_id)
    }

    pub fn get_mut(&mut self, document_id: u64) -> Option<&mut EditorState> {
        self.documents.get_mut(&document_id)
    }
}

/// 全局文档注册表。当前 UI 仍使用 active document，后端已支持按 documentId 隔离状态。
static DOCUMENT_REGISTRY: OnceLock<Arc<RwLock<DocumentRegistry>>> = OnceLock::new();

pub fn get_registry() -> Arc<RwLock<DocumentRegistry>> {
    DOCUMENT_REGISTRY
        .get_or_init(|| Arc::new(RwLock::new(DocumentRegistry::new())))
        .clone()
}
