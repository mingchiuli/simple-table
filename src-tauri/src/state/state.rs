use std::sync::{Arc, OnceLock, RwLock};

use crate::error::AppError;
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
    #[serde(default)]
    pub history: HistoryStatus,
}

#[derive(serde::Serialize, serde::Deserialize, TS, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct HistoryStatus {
    pub is_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
    pub undo_entries: usize,
    pub redo_entries: usize,
    pub undo_estimated_bytes: usize,
    pub redo_estimated_bytes: usize,
    pub max_history_bytes: usize,
    pub max_single_entry_bytes: usize,
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

    pub fn try_replace_active(&mut self, editor_state: EditorState) -> Result<u64, AppError> {
        if self
            .active
            .as_ref()
            .is_some_and(EditorState::has_save_commit_in_progress)
        {
            return Err(AppError::DocumentStateInvalid(
                "cannot replace the active document while save is in progress".to_string(),
            ));
        }
        Ok(self.replace_active(editor_state))
    }

    pub fn close_active(&mut self) -> Result<Option<u64>, AppError> {
        if self
            .active
            .as_ref()
            .is_some_and(EditorState::has_save_commit_in_progress)
        {
            return Err(AppError::DocumentStateInvalid(
                "cannot close the active document while save is in progress".to_string(),
            ));
        }
        Ok(self
            .active
            .take()
            .map(|editor_state| editor_state.document_id()))
    }

    pub fn active(&self) -> Option<&EditorState> {
        self.active.as_ref()
    }

    pub fn active_mut(&mut self) -> Option<&mut EditorState> {
        self.active.as_mut()
    }

    pub fn active_mut_for_command(
        &mut self,
        document_id: u64,
        base_revision: u64,
    ) -> Result<&mut EditorState, AppError> {
        let editor_state = self.active.as_mut().ok_or(AppError::NoFileLoaded)?;
        if editor_state.document_id() != document_id {
            return Err(AppError::DocumentStateInvalid(
                "active document changed before the editor command was applied".to_string(),
            ));
        }
        if editor_state.revision() != base_revision {
            return Err(AppError::DocumentStateInvalid(format!(
                "document revision changed before the editor command was applied: expected {}, got {}",
                base_revision,
                editor_state.revision()
            )));
        }
        Ok(editor_state)
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
    Arc::clone(
        ACTIVE_DOCUMENT_STORE.get_or_init(|| Arc::new(RwLock::new(ActiveDocumentStore::new()))),
    )
}
