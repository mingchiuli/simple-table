import { useDocumentSessionStore } from "@/stores/documentSession";
import { useRecentFilesStore } from "@/stores/recentFiles";
import type { EditorCommandContext } from "@/types";
import {
  tryAddRecentFileWithThumbnail,
  tryRefreshRecentFiles,
} from "@/utils/recentFileTracking";

export function useRecentFileUpdates() {
  const documentSessionStore = useDocumentSessionStore();
  const recentFilesStore = useRecentFilesStore();

  function queueRecentFileEntryUpdate(originalPath?: string) {
    const context = documentSessionStore.currentCommandContext();
    void updateRecentFileEntry(originalPath, context);
  }

  async function updateRecentFileEntry(
    originalPath: string | undefined,
    context: EditorCommandContext | null
  ) {
    if (!context) {
      return;
    }
    await tryAddRecentFileWithThumbnail({ originalPath, context });
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
