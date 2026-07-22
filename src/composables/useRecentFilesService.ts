import type { RecentFilesService } from '@/application/recentFilesService';
import { useApplicationWorkspaceRuntime } from '@/composables/applicationWorkspaceRuntime';

export function useRecentFilesService(): RecentFilesService {
  return useApplicationWorkspaceRuntime().recentFiles;
}
