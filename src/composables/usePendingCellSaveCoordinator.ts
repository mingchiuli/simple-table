import { useDocumentWorkspaceRuntime } from '@/composables/documentWorkspaceRuntime';

export function usePendingCellSaveCoordinator() {
  return useDocumentWorkspaceRuntime().pendingCellSaves;
}
