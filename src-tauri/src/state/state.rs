use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};

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
    #[serde(with = "crate::types::u64_string")]
    #[ts(type = "U64String")]
    pub document_id: u64,
    #[serde(with = "crate::types::u64_string")]
    #[ts(type = "U64String")]
    pub revision: u64,
    pub formula_status: FormulaStatus,
    pub capabilities: WorkbookCapabilities,
    pub editor_state: EditorStateInfo,
}

pub struct ActiveDocumentStore {
    active: Option<Arc<DocumentHandle>>,
    replacement_lease: Option<u64>,
}

pub struct DocumentHandle {
    document_id: u64,
    retired: AtomicBool,
    state: RwLock<EditorState>,
}

impl DocumentHandle {
    fn new(editor_state: EditorState) -> Self {
        Self {
            document_id: editor_state.document_id(),
            retired: AtomicBool::new(false),
            state: RwLock::new(editor_state),
        }
    }

    pub(crate) fn document_id(&self) -> u64 {
        self.document_id
    }

    #[cfg(test)]
    pub(crate) fn revision(&self) -> u64 {
        self.read().expect("document state").revision()
    }

    #[cfg(test)]
    pub(crate) fn search_sheet_index_stamp(
        &self,
        sheet_index: usize,
    ) -> crate::state::search_index::SearchIndexStamp {
        self.read()
            .expect("document state")
            .search_sheet_index_stamp(sheet_index)
    }

    #[cfg(test)]
    pub(crate) fn indexed_search_sheet(
        &self,
        sheet_index: usize,
    ) -> Option<Arc<crate::state::search_index::SearchSheetIndex>> {
        self.read()
            .expect("document state")
            .indexed_search_sheet(sheet_index)
    }

    #[cfg(test)]
    pub(crate) fn search_sheet_data(&self, sheet_index: usize) -> Option<crate::types::SheetData> {
        self.read()
            .expect("document state")
            .search_sheet_data(sheet_index)
    }

    pub(crate) fn read(&self) -> Result<RwLockReadGuard<'_, EditorState>, AppError> {
        let guard = self
            .state
            .read()
            .map_err(|_| AppError::poisoned_lock("document state"))?;
        self.ensure_active()?;
        Ok(guard)
    }

    pub(crate) fn read_for_command(
        &self,
        document_id: u64,
        base_revision: u64,
    ) -> Result<RwLockReadGuard<'_, EditorState>, AppError> {
        let guard = self.read()?;
        validate_command_context(&guard, document_id, base_revision)?;
        Ok(guard)
    }

    pub(crate) fn write(&self) -> Result<RwLockWriteGuard<'_, EditorState>, AppError> {
        let guard = self
            .state
            .write()
            .map_err(|_| AppError::poisoned_lock("document state"))?;
        self.ensure_active()?;
        Ok(guard)
    }

    pub(crate) fn write_for_command(
        &self,
        document_id: u64,
        base_revision: u64,
    ) -> Result<RwLockWriteGuard<'_, EditorState>, AppError> {
        let guard = self.write()?;
        validate_command_context(&guard, document_id, base_revision)?;
        Ok(guard)
    }

    fn retire(&self) -> Result<(), AppError> {
        let state = self
            .state
            .write()
            .map_err(|_| AppError::poisoned_lock("document state"))?;
        if state.has_save_commit_in_progress() {
            return Err(AppError::DocumentStateInvalid(
                "cannot retire the active document while save is in progress".to_string(),
            ));
        }
        self.retired.store(true, Ordering::Release);
        Ok(())
    }

    fn ensure_active(&self) -> Result<(), AppError> {
        if !self.retired.load(Ordering::Acquire) {
            return Ok(());
        }
        Err(AppError::DocumentStateInvalid(
            "document is no longer active".to_string(),
        ))
    }
}

fn validate_command_context(
    editor_state: &EditorState,
    document_id: u64,
    base_revision: u64,
) -> Result<(), AppError> {
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
    Ok(())
}

impl ActiveDocumentStore {
    fn new() -> Self {
        Self {
            active: None,
            replacement_lease: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test() -> Self {
        Self::new()
    }

    fn replace_active(&mut self, editor_state: EditorState) -> (u64, Option<Arc<DocumentHandle>>) {
        let document_id = editor_state.document_id();
        let previous = self
            .active
            .replace(Arc::new(DocumentHandle::new(editor_state)));
        (document_id, previous)
    }

    #[cfg(test)]
    pub(crate) fn replace_active_for_test(&mut self, editor_state: EditorState) -> u64 {
        let (document_id, previous) = self.replace_active(editor_state);
        if let Some(previous) = &previous {
            previous.retire().expect("retire previous test document");
        }
        drop(previous);
        document_id
    }

    #[cfg(test)]
    pub(crate) fn replace_active_for_context<T>(
        &mut self,
        expected_document_id: Option<u64>,
        expected_revision: Option<u64>,
        load_prepared: impl FnOnce() -> Result<(EditorState, T), AppError>,
    ) -> Result<(u64, Option<u64>, T), AppError> {
        self.ensure_replacement_context(expected_document_id, expected_revision)?;
        let (editor_state, metadata) = load_prepared()?;
        let (document_id, previous) = self.replace_active(editor_state);
        if let Some(previous) = &previous {
            previous.retire()?;
        }
        let previous_document_id = previous.as_ref().map(|handle| handle.document_id());
        drop(previous);
        Ok((document_id, previous_document_id, metadata))
    }

    pub(crate) fn ensure_replacement_context(
        &self,
        document_id: Option<u64>,
        revision: Option<u64>,
    ) -> Result<(), AppError> {
        if self.replacement_lease.is_some() {
            return Err(AppError::DocumentStateInvalid(
                "another document replacement is already in progress".to_string(),
            ));
        }
        match (document_id, revision) {
            (Some(document_id), Some(revision)) => {
                let handle = self.active.as_ref().ok_or(AppError::NoFileLoaded)?;
                let editor_state = handle.read()?;
                if editor_state.document_id() != document_id || editor_state.revision() != revision
                {
                    return Err(AppError::DocumentStateInvalid(
                        "active document changed before the prepared document was committed"
                            .to_string(),
                    ));
                }
                if editor_state.has_save_commit_in_progress() {
                    return Err(AppError::DocumentStateInvalid(
                        "cannot replace the active document while save is in progress".to_string(),
                    ));
                }
                Ok(())
            }
            (None, None) if self.active.is_none() => Ok(()),
            (None, None) => Err(AppError::DocumentStateInvalid(
                "an active backend document exists but the replacement request did not identify it"
                    .to_string(),
            )),
            _ => Err(AppError::DocumentStateInvalid(
                "prepared document commit must include both expected documentId and revision"
                    .to_string(),
            )),
        }
    }

    fn close_active(&mut self) -> Result<Option<Arc<DocumentHandle>>, AppError> {
        if let Some(handle) = &self.active {
            handle.retire()?;
        }
        Ok(self.active.take())
    }

    pub(crate) fn close_active_document(
        &mut self,
        document_id: u64,
    ) -> Result<Option<Arc<DocumentHandle>>, AppError> {
        self.ensure_no_replacement_in_progress()?;
        let handle = self.active.as_ref().ok_or(AppError::NoFileLoaded)?;
        if handle.document_id() != document_id {
            return Err(AppError::DocumentStateInvalid(
                "active document changed before it was closed".to_string(),
            ));
        }
        self.close_active()
    }

    pub(crate) fn active_handle(&self) -> Option<Arc<DocumentHandle>> {
        self.active.as_ref().map(Arc::clone)
    }

    #[cfg(test)]
    pub(crate) fn active(&self) -> Option<Arc<DocumentHandle>> {
        self.active_handle()
    }

    pub(crate) fn active_handle_for_mutation(
        &self,
        document_id: u64,
    ) -> Result<Arc<DocumentHandle>, AppError> {
        self.ensure_no_replacement_in_progress()?;
        let handle = self.active.as_ref().ok_or(AppError::NoFileLoaded)?;
        if handle.document_id() != document_id {
            return Err(AppError::DocumentStateInvalid(
                "active document changed before the editor command was applied".to_string(),
            ));
        }
        Ok(Arc::clone(handle))
    }

    pub(crate) fn active_handle_for_read(
        &self,
        document_id: u64,
    ) -> Result<Arc<DocumentHandle>, AppError> {
        let handle = self.active.as_ref().ok_or(AppError::NoFileLoaded)?;
        if handle.document_id() != document_id {
            return Err(AppError::DocumentStateInvalid(
                "active document changed before the editor command was applied".to_string(),
            ));
        }
        Ok(Arc::clone(handle))
    }

    pub(crate) fn handle(&self, document_id: u64) -> Option<Arc<DocumentHandle>> {
        self.active
            .as_ref()
            .filter(|handle| handle.document_id() == document_id)
            .map(Arc::clone)
    }

    #[cfg(test)]
    pub(crate) fn get(&self, document_id: u64) -> Option<Arc<DocumentHandle>> {
        self.handle(document_id)
    }

    #[cfg(test)]
    pub(crate) fn active_for_command(
        &self,
        document_id: u64,
        base_revision: u64,
    ) -> Result<Arc<DocumentHandle>, AppError> {
        let handle = self.active_handle_for_read(document_id)?;
        drop(handle.read_for_command(document_id, base_revision)?);
        Ok(handle)
    }

    #[cfg(test)]
    pub(crate) fn active_mut_for_command(
        &self,
        document_id: u64,
        base_revision: u64,
    ) -> Result<Arc<DocumentHandle>, AppError> {
        let handle = self.active_handle_for_mutation(document_id)?;
        drop(handle.write_for_command(document_id, base_revision)?);
        Ok(handle)
    }

    #[cfg(test)]
    pub(crate) fn active_mut_for_save(
        &self,
        document_id: u64,
    ) -> Result<Arc<DocumentHandle>, AppError> {
        let handle = self.active_handle_for_mutation(document_id)?;
        drop(handle.write()?);
        Ok(handle)
    }

    pub(crate) fn begin_document_replacement(
        &mut self,
        expected_document_id: Option<u64>,
        expected_revision: Option<u64>,
    ) -> Result<DocumentReplacementLease, AppError> {
        self.ensure_replacement_context(expected_document_id, expected_revision)?;
        let lease = DocumentReplacementLease(nonzero_random_u64());
        self.replacement_lease = Some(lease.0);
        Ok(lease)
    }

    pub(crate) fn finish_document_replacement(
        &mut self,
        lease: DocumentReplacementLease,
        editor_state: EditorState,
    ) -> Result<(u64, Option<Arc<DocumentHandle>>), AppError> {
        self.ensure_replacement_lease(lease)?;
        if let Some(previous) = &self.active {
            previous.retire()?;
        }
        let (document_id, previous) = self.replace_active(editor_state);
        self.replacement_lease = None;
        Ok((document_id, previous))
    }

    pub(crate) fn abort_document_replacement(&mut self, lease: DocumentReplacementLease) {
        if self.replacement_lease == Some(lease.0) {
            self.replacement_lease = None;
        }
    }

    fn ensure_no_replacement_in_progress(&self) -> Result<(), AppError> {
        if self.replacement_lease.is_none() {
            return Ok(());
        }
        Err(AppError::DocumentStateInvalid(
            "document replacement is in progress".to_string(),
        ))
    }

    fn ensure_replacement_lease(&self, lease: DocumentReplacementLease) -> Result<(), AppError> {
        if self.replacement_lease == Some(lease.0) {
            return Ok(());
        }
        Err(AppError::DocumentStateInvalid(
            "document replacement lease is no longer active".to_string(),
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DocumentReplacementLease(u64);

fn nonzero_random_u64() -> u64 {
    loop {
        let value = uuid::Uuid::new_v4().as_u128() as u64;
        if value != 0 {
            return value;
        }
    }
}

/// 全局活动文档状态。Simple Table 当前是单文档 UI；documentId 只用于丢弃过期异步任务。
static ACTIVE_DOCUMENT_STORE: OnceLock<Arc<RwLock<ActiveDocumentStore>>> = OnceLock::new();

pub(crate) fn active_document_store() -> Arc<RwLock<ActiveDocumentStore>> {
    Arc::clone(
        ACTIVE_DOCUMENT_STORE.get_or_init(|| Arc::new(RwLock::new(ActiveDocumentStore::new()))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FileData, SheetData};

    fn editor_state(name: &str) -> EditorState {
        EditorState::with_workbook(
            FileData {
                path: String::new(),
                file_name: name.to_string(),
                sheets: vec![SheetData::default()],
            },
            None,
        )
    }

    #[test]
    fn replacement_context_accepts_an_empty_store_without_tokens() {
        let store = ActiveDocumentStore::new_for_test();

        assert!(store.ensure_replacement_context(None, None).is_ok());
    }

    #[test]
    fn replacement_context_requires_the_active_document_and_revision() {
        let mut store = ActiveDocumentStore::new_for_test();
        let state = editor_state("current.xlsx");
        let document_id = state.document_id();
        let revision = state.revision();
        store.replace_active_for_test(state);

        assert!(
            store
                .ensure_replacement_context(Some(document_id), Some(revision))
                .is_ok()
        );
        assert!(
            store
                .ensure_replacement_context(Some(document_id), Some(revision + 1))
                .is_err()
        );
        assert!(store.ensure_replacement_context(None, None).is_err());
        assert_eq!(
            store.active().map(|handle| handle.document_id()),
            Some(document_id)
        );
    }

    #[test]
    fn replacement_context_rejects_a_document_with_save_commit_in_progress() {
        let mut store = ActiveDocumentStore::new_for_test();
        let mut state = editor_state("current.xlsx");
        let document_id = state.document_id();
        let revision = state.revision();
        state
            .begin_save_commit(document_id, revision)
            .expect("begin save commit");
        store.replace_active_for_test(state);

        assert!(
            store
                .ensure_replacement_context(Some(document_id), Some(revision))
                .is_err()
        );
        assert_eq!(
            store.active().map(|handle| handle.document_id()),
            Some(document_id)
        );
    }

    #[test]
    fn contextual_replacement_does_not_load_or_replace_on_stale_context() {
        let mut store = ActiveDocumentStore::new_for_test();
        let current = editor_state("current.xlsx");
        let current_document_id = current.document_id();
        let current_revision = current.revision();
        store.replace_active_for_test(current);
        let mut loaded = false;

        let result = store.replace_active_for_context(
            Some(current_document_id),
            Some(current_revision + 1),
            || {
                loaded = true;
                Ok((editor_state("next.xlsx"), ()))
            },
        );

        assert!(result.is_err());
        assert!(!loaded);
        assert_eq!(
            store.active().map(|handle| handle.document_id()),
            Some(current_document_id)
        );
    }

    #[test]
    fn replacement_lease_blocks_mutation_and_close_until_aborted() {
        let mut store = ActiveDocumentStore::new_for_test();
        let state = editor_state("current.xlsx");
        let document_id = state.document_id();
        let revision = state.revision();
        store.replace_active_for_test(state);
        let lease = store
            .begin_document_replacement(Some(document_id), Some(revision))
            .expect("replacement lease");

        assert!(store.active_for_command(document_id, revision).is_ok());
        assert!(store.active_mut_for_command(document_id, revision).is_err());
        assert!(store.active_mut_for_save(document_id).is_err());
        assert!(store.close_active_document(document_id).is_err());

        store.abort_document_replacement(lease);
        assert!(store.active_mut_for_command(document_id, revision).is_ok());
    }

    #[test]
    fn replacement_lease_atomically_installs_the_prepared_document() {
        let mut store = ActiveDocumentStore::new_for_test();
        let current = editor_state("current.xlsx");
        let current_id = current.document_id();
        let current_revision = current.revision();
        store.replace_active_for_test(current);
        let replacement = editor_state("replacement.xlsx");
        let replacement_id = replacement.document_id();
        let lease = store
            .begin_document_replacement(Some(current_id), Some(current_revision))
            .expect("replacement lease");

        let (document_id, previous) = store
            .finish_document_replacement(lease, replacement)
            .expect("finish replacement");

        assert_eq!(document_id, replacement_id);
        assert_eq!(
            previous.as_ref().map(|handle| handle.document_id()),
            Some(current_id)
        );
        assert_eq!(
            store.active().map(|handle| handle.document_id()),
            Some(replacement_id)
        );
        drop(previous);
    }

    #[test]
    fn close_detaches_the_document_for_release_after_the_store_borrow() {
        let mut store = ActiveDocumentStore::new_for_test();
        let current = editor_state("current.xlsx");
        let current_id = current.document_id();
        store.replace_active_for_test(current);

        let detached = store
            .close_active_document(current_id)
            .expect("close document")
            .expect("detached document");

        assert!(store.active().is_none());
        assert_eq!(detached.document_id(), current_id);
        assert!(detached.read().is_err());
        drop(detached);
    }

    #[test]
    fn document_content_lock_does_not_hold_the_registry_lock() {
        let mut store = ActiveDocumentStore::new_for_test();
        store.replace_active_for_test(editor_state("current.xlsx"));
        let registry = RwLock::new(store);
        let handle = registry
            .read()
            .unwrap()
            .active_handle()
            .expect("active document");

        let _document_guard = handle.write().expect("document state");

        assert!(registry.try_write().is_ok());
    }

    #[test]
    fn replacement_retires_handles_cloned_before_the_swap() {
        let mut store = ActiveDocumentStore::new_for_test();
        let current = editor_state("current.xlsx");
        let current_id = current.document_id();
        let current_revision = current.revision();
        store.replace_active_for_test(current);
        let stale_handle = store.active_handle().expect("stale handle");
        let lease = store
            .begin_document_replacement(Some(current_id), Some(current_revision))
            .expect("replacement lease");

        store
            .finish_document_replacement(lease, editor_state("next.xlsx"))
            .expect("finish replacement");

        assert!(stale_handle.read().is_err());
        assert!(stale_handle.write().is_err());
    }

    #[test]
    fn contextual_replacement_keeps_active_document_when_loading_token_fails() {
        let mut store = ActiveDocumentStore::new_for_test();
        let current = editor_state("current.xlsx");
        let current_document_id = current.document_id();
        let current_revision = current.revision();
        store.replace_active_for_test(current);

        let result = store.replace_active_for_context::<()>(
            Some(current_document_id),
            Some(current_revision),
            || {
                Err(AppError::DocumentStateInvalid(
                    "prepared token expired".to_string(),
                ))
            },
        );

        assert!(result.is_err());
        assert_eq!(
            store.active().map(|handle| handle.document_id()),
            Some(current_document_id)
        );
    }

    #[test]
    fn contextual_replacement_commits_after_context_and_token_validation() {
        let mut store = ActiveDocumentStore::new_for_test();
        let current = editor_state("current.xlsx");
        let current_document_id = current.document_id();
        let current_revision = current.revision();
        store.replace_active_for_test(current);
        let next = editor_state("next.xlsx");
        let next_document_id = next.document_id();

        let (document_id, previous_document_id, metadata) = store
            .replace_active_for_context(Some(current_document_id), Some(current_revision), || {
                Ok((next, "prepared metadata"))
            })
            .expect("commit prepared document");

        assert_eq!(document_id, next_document_id);
        assert_eq!(previous_document_id, Some(current_document_id));
        assert_eq!(metadata, "prepared metadata");
        assert_eq!(
            store.active().map(|handle| handle.document_id()),
            Some(next_document_id)
        );
    }
}
