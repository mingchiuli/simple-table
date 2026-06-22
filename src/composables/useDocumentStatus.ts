import * as api from '@/api';

export function useDocumentStatus() {
  const canUndo = ref(false);
  const canRedo = ref(false);
  const isContentDirty = ref(false);
  const hasPendingContentChange = ref(false);

  const hasUnsavedChanges = computed(() => isContentDirty.value || hasPendingContentChange.value);

  async function refreshEditorState() {
    try {
      const state = await api.getEditorState();
      canUndo.value = state?.canUndo ?? false;
      canRedo.value = state?.canRedo ?? false;
      isContentDirty.value = state?.isDirty ?? false;
    } catch (error) {
      console.error('Failed to get editor state:', error);
    }
  }

  function markPendingContentChange() {
    hasPendingContentChange.value = true;
  }

  function clearPendingContentChange() {
    hasPendingContentChange.value = false;
  }

  function resetDocumentStatus() {
    canUndo.value = false;
    canRedo.value = false;
    isContentDirty.value = false;
    hasPendingContentChange.value = false;
  }

  async function markSaved() {
    await api.markFileSaved();
    hasPendingContentChange.value = false;
    await refreshEditorState();
  }

  return {
    canUndo,
    canRedo,
    hasUnsavedChanges,
    isContentDirty,
    hasPendingContentChange,
    refreshEditorState,
    markPendingContentChange,
    clearPendingContentChange,
    resetDocumentStatus,
    markSaved,
  };
}
