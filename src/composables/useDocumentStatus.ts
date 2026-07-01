import * as api from '@/api';
import { useDocumentSessionStore } from '@/stores/documentSession';
import { useDocumentStatusStore } from '@/stores/documentStatus';
import type { EditorSessionInfo, EditorStateInfo } from '@/types';

export function useDocumentStatus() {
  const documentSessionStore = useDocumentSessionStore();
  const documentStatusStore = useDocumentStatusStore();
  const {
    canUndo,
    canRedo,
    isContentDirty,
    hasPendingContentChange,
    hasUnsavedChanges,
    formulaStatus,
    capabilities,
  } = storeToRefs(documentStatusStore);

  function applyEditorState(state: EditorStateInfo | null | undefined) {
    documentStatusStore.applyEditorState(state);
  }

  function applyEditorSession(info: EditorSessionInfo | null | undefined) {
    documentSessionStore.applyEditorSession(info);
  }

  async function refreshEditorState() {
    try {
      const session = await api.getEditorState();
      applyEditorSession(session);
    } catch (error) {
      console.error('Failed to get editor state:', error);
    }
  }

  function markPendingContentChange() {
    documentStatusStore.markPendingContentChange();
  }

  function clearPendingContentChange() {
    documentStatusStore.clearPendingContentChange();
  }

  function resetDocumentStatus() {
    documentStatusStore.reset();
  }

  async function markSaved() {
    await api.markFileSaved();
    documentStatusStore.clearPendingContentChange();
    await refreshEditorState();
  }

  return {
    canUndo,
    canRedo,
    hasUnsavedChanges,
    formulaStatus,
    capabilities,
    isContentDirty,
    hasPendingContentChange,
    applyEditorState,
    applyEditorSession,
    refreshEditorState,
    markPendingContentChange,
    clearPendingContentChange,
    resetDocumentStatus,
    markSaved,
  };
}
