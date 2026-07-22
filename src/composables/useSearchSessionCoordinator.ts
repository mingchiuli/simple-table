import { useDocumentWorkspaceRuntime } from '@/composables/documentWorkspaceRuntime';

export function useSearchSessionCoordinator() {
  return useDocumentWorkspaceRuntime().search;
}
