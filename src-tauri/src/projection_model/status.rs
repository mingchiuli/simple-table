use crate::document::capabilities::WorkbookCapabilities;
use crate::formula::status::FormulaStatus;
use crate::state::history_store::HistoryStatus;

#[derive(Clone, Debug)]
pub(crate) struct EditorStateSnapshot {
    pub can_undo: bool,
    pub can_redo: bool,
    pub is_dirty: bool,
    pub history: HistoryStatus,
}

#[derive(Clone, Debug)]
pub(crate) struct EditorSessionSnapshot {
    pub document_id: u64,
    pub revision: u64,
    pub formula_status: FormulaStatus,
    pub capabilities: WorkbookCapabilities,
    pub editor_state: EditorStateSnapshot,
}
