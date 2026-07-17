import * as api from '@/api';
import { useDocumentSessionCoordinator } from '@/composables/useDocumentSessionCoordinator';
import { useDocumentSessionStore } from '@/stores/documentSession';
import { useDocumentStatusStore } from '@/stores/documentStatus';

export function useDocumentStatus() {
  const documentSessionStore = useDocumentSessionStore();
  const documentSessionCoordinator = useDocumentSessionCoordinator();
  const documentStatusStore = useDocumentStatusStore();
  const {
    canUndo,
    canRedo,
    isContentDirty,
    hasPendingContentChange,
    hasUnsavedChanges,
    formulaStatus,
    capabilities,
    history,
  } = storeToRefs(documentStatusStore);

  async function refreshEditorState() {
    const context = documentSessionStore.currentCommandContext();
    try {
      const session = await api.getEditorState(context);
      documentSessionCoordinator.applyEditorSessionForContext(context, session);
    } catch (error) {
      if (!context || documentSessionStore.matchesCommandContext(context)) {
        console.error('Failed to get editor state:', error);
      }
    }
  }

  return {
    canUndo,
    canRedo,
    hasUnsavedChanges,
    formulaStatus,
    capabilities,
    history,
    isContentDirty,
    hasPendingContentChange,
    refreshEditorState,
  };
}
