import { useDocumentWorkspaceRuntime } from '@/composables/documentWorkspaceRuntime';

export function useDocumentCommandBus() {
  return useDocumentWorkspaceRuntime().commandBus;
}
