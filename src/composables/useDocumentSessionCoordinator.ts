import { useDocumentWorkspaceRuntime } from '@/composables/documentWorkspaceRuntime';

export function useDocumentSessionCoordinator() {
  return useDocumentWorkspaceRuntime().session;
}
