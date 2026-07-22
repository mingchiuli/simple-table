import type { UpdateCoordinator } from '@/application/updateCoordinator';
import { useApplicationWorkspaceRuntime } from '@/composables/applicationWorkspaceRuntime';

export function useUpdateCoordinator(): UpdateCoordinator {
  return useApplicationWorkspaceRuntime().updates;
}
