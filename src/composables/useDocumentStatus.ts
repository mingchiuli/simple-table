import * as api from '@/api';
import { useDocumentSessionStore } from '@/stores/documentSession';
import type { EditorSessionInfo, EditorStateInfo } from '@/types';

export function useDocumentStatus() {
  const documentSessionStore = useDocumentSessionStore();
  const {
    canUndo,
    canRedo,
    isContentDirty,
    hasPendingContentChange,
    hasUnsavedChanges,
  } = storeToRefs(documentSessionStore);

  function applyEditorState(state: EditorStateInfo | null | undefined) {
    documentSessionStore.applyEditorState(state);
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
    documentSessionStore.markPendingContentChange();
  }

  function clearPendingContentChange() {
    documentSessionStore.clearPendingContentChange();
  }

  function resetDocumentStatus() {
    documentSessionStore.resetDocumentStatus();
  }

  async function markSaved() {
    await api.markFileSaved();
    documentSessionStore.clearPendingContentChange();
    await refreshEditorState();
  }

  return {
    canUndo,
    canRedo,
    hasUnsavedChanges,
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
