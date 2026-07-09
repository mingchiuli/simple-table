import { getStorageType } from "@/platform";
import { useDocumentSessionStore } from "@/stores/documentSession";
import { useRecentFilesStore } from "@/stores/recentFiles";
import type { EditorCommandContext } from "@/types";
import {
  tryAddRecentFileWithResolvedStorage,
  tryRefreshRecentFiles,
} from "@/utils/recentFileTracking";

export function useRecentFileUpdates() {
  const documentSessionStore = useDocumentSessionStore();
  const recentFilesStore = useRecentFilesStore();

  function queueRecentFileEntryUpdate(
    path: string,
    fileName: string,
    originalPath?: string
  ) {
    const context = documentSessionStore.currentCommandContext();
    void updateRecentFileEntry(path, fileName, originalPath, context);
  }

  async function updateRecentFileEntry(
    path: string,
    fileName: string,
    originalPath: string | undefined,
    context: EditorCommandContext | null
  ) {
    await tryAddRecentFileWithResolvedStorage(
      {
        path,
        fileName,
        originalPath,
        context,
      },
      getStorageType
    );
    await refreshRecentFiles();
  }

  async function refreshRecentFiles() {
    await tryRefreshRecentFiles(() => recentFilesStore.load());
  }

  return {
    queueRecentFileEntryUpdate,
    refreshRecentFiles,
  };
}
