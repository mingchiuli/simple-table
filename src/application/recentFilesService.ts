import type { EditorCommandContext } from '@/types/documentRuntime';
import type { RecentFile } from '@/types/recentFileRuntime';

export type RecentFilesPort = {
  getRecentFiles(): Promise<RecentFile[]>;
  removeRecentFile(id: string): Promise<void>;
  addRecentFileWithThumbnail(
    context: EditorCommandContext,
    originalPath?: string,
  ): Promise<void>;
};

export type RecentFilesState = {
  replaceFiles(files: RecentFile[]): void;
  setLoading(loading: boolean): void;
};

export type RecentFileTrackingRequest = {
  originalPath?: string;
  context: EditorCommandContext;
};

type RecentFilesRuntime = {
  loadRequestId: number;
  activeLoadCount: number;
  activeTracking: Promise<void> | null;
  pendingTracking: RecentFileTrackingRequest | null;
};

export type RecentFilesService = ReturnType<typeof createRecentFilesService>;

export function createRecentFilesService(
  store: RecentFilesState,
  port: RecentFilesPort,
  reportFailure: (error: unknown) => void = () => undefined,
) {
  const runtime: RecentFilesRuntime = {
    loadRequestId: 0,
    activeLoadCount: 0,
    activeTracking: null,
    pendingTracking: null,
  };

  async function load() {
    const requestId = runtime.loadRequestId + 1;
    runtime.loadRequestId = requestId;
    runtime.activeLoadCount += 1;
    store.setLoading(true);
    try {
      const files = await port.getRecentFiles();
      if (requestId === runtime.loadRequestId) {
        store.replaceFiles(files);
      }
    } finally {
      runtime.activeLoadCount = Math.max(0, runtime.activeLoadCount - 1);
      store.setLoading(runtime.activeLoadCount > 0);
    }
  }

  async function refresh(): Promise<boolean> {
    try {
      await load();
      return true;
    } catch (error) {
      safeReportFailure(error);
      return false;
    }
  }

  async function remove(id: string) {
    await port.removeRecentFile(id);
    await load();
  }

  function queueRecentFileEntryUpdate(request: RecentFileTrackingRequest) {
    runtime.pendingTracking = request;
    startTrackingWorker();
  }

  function startTrackingWorker() {
    if (runtime.activeTracking) return;
    const worker = runTrackingWorker().catch(safeReportFailure);
    runtime.activeTracking = worker;
    void worker.finally(() => {
      if (runtime.activeTracking === worker) runtime.activeTracking = null;
      if (runtime.pendingTracking) startTrackingWorker();
    });
  }

  async function runTrackingWorker() {
    while (true) {
      while (runtime.pendingTracking) {
        const request = runtime.pendingTracking;
        runtime.pendingTracking = null;
        try {
          await port.addRecentFileWithThumbnail(request.context, request.originalPath);
        } catch (error) {
          safeReportFailure(error);
        }
      }
      await refresh();
      if (!runtime.pendingTracking) return;
    }
  }

  function safeReportFailure(error: unknown) {
    try {
      reportFailure(error);
    } catch {
      // Failure reporting must not reject a background metadata worker.
    }
  }

  return { load, refresh, remove, queueRecentFileEntryUpdate };
}
