import { useRecentFilesService } from '@/composables/useRecentFilesService';
import { useDocumentSessionStore } from '@/stores/documentSession';

export function useRecentFileUpdates() {
  const documentSessionStore = useDocumentSessionStore();
  const recentFilesService = useRecentFilesService();

  function queueRecentFileEntryUpdate(originalPath?: string) {
    const context = documentSessionStore.currentCommandContext();
    if (!context) return;
    recentFilesService.queueRecentFileEntryUpdate({ originalPath, context });
  }

  return {
    queueRecentFileEntryUpdate,
    refreshRecentFiles: recentFilesService.refresh,
    removeRecentFile: recentFilesService.remove,
  };
}
