import { useRecentFilesService } from '@/composables/useRecentFilesService';
import type { FileOperationReceipt } from '@/types/fileRuntime';

export function useRecentFileUpdates() {
  const recentFilesService = useRecentFilesService();

  function queueRecentFileEntryUpdate(
    receipt: FileOperationReceipt,
    originalPath?: string,
  ) {
    recentFilesService.queueRecentFileEntryUpdate({ originalPath, receipt });
  }

  return {
    queueRecentFileEntryUpdate,
    refreshRecentFiles: recentFilesService.refresh,
    removeRecentFile: recentFilesService.remove,
  };
}
