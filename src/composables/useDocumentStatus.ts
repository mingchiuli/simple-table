import { useDocumentStatusStore } from '@/stores/documentStatus';

export function useDocumentStatus() {
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

  return {
    canUndo,
    canRedo,
    hasUnsavedChanges,
    formulaStatus,
    capabilities,
    history,
    isContentDirty,
    hasPendingContentChange,
  };
}
