import * as api from '@/api';
import { useDocumentSessionStore } from '@/stores/documentSession';
import type { EditorStateInfo } from '@/types';

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

  async function refreshEditorState() {
    try {
      const state = await api.getEditorState();
      applyEditorState(state);
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
    refreshEditorState,
    markPendingContentChange,
    clearPendingContentChange,
    resetDocumentStatus,
    markSaved,
  };
}
