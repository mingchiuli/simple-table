import { useDocumentSessionStore } from "@/stores/documentSession";
import { useRecentFilesStore } from "@/stores/recentFiles";
import { useRecentFilesService } from "@/application/recentFilesService";
import type { EditorCommandContext } from "@/types";
import {
  tryAddRecentFileWithThumbnail,
  tryRefreshRecentFiles,
} from "@/utils/recentFileTracking";

type RecentFileUpdateRequest = {
  originalPath: string | undefined;
  context: EditorCommandContext;
};

type RecentFileUpdateScheduler = {
  active: Promise<void> | null;
  pending: RecentFileUpdateRequest | null;
};

const recentFileUpdateSchedulers = new WeakMap<object, RecentFileUpdateScheduler>();

export function useRecentFileUpdates() {
  const documentSessionStore = useDocumentSessionStore();
  const recentFilesStore = useRecentFilesStore();
  const recentFilesService = useRecentFilesService();

  function queueRecentFileEntryUpdate(originalPath?: string) {
    const context = documentSessionStore.currentCommandContext();
    if (!context) return;
    const scheduler = recentFileUpdateSchedulerFor(recentFilesStore);
    scheduler.pending = { originalPath, context };
    startRecentFileUpdateWorker(scheduler);
  }

  function startRecentFileUpdateWorker(scheduler: RecentFileUpdateScheduler) {
    if (scheduler.active) return;
    scheduler.active = runRecentFileUpdateWorker(scheduler)
      .catch((error) => {
        console.error("Recent file update worker failed:", error);
      })
      .finally(() => {
        scheduler.active = null;
        if (scheduler.pending) startRecentFileUpdateWorker(scheduler);
      });
  }

  async function runRecentFileUpdateWorker(scheduler: RecentFileUpdateScheduler) {
    while (true) {
      while (scheduler.pending) {
        const request = scheduler.pending;
        scheduler.pending = null;
        await tryAddRecentFileWithThumbnail(request);
      }
      await refreshRecentFiles();
      if (!scheduler.pending) return;
    }
  }

  async function refreshRecentFiles() {
    await tryRefreshRecentFiles(recentFilesService.load);
  }

  return {
    queueRecentFileEntryUpdate,
    refreshRecentFiles,
    removeRecentFile: recentFilesService.remove,
  };
}

function recentFileUpdateSchedulerFor(store: object): RecentFileUpdateScheduler {
  let scheduler = recentFileUpdateSchedulers.get(store);
  if (!scheduler) {
    scheduler = { active: null, pending: null };
    recentFileUpdateSchedulers.set(store, scheduler);
  }
  return scheduler;
}
