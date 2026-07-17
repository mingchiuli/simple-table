import type { EditorCommandContext, RecentFile } from '@/types';

export type RecentFilesPort = {
  getRecentFiles(): Promise<RecentFile[]>;
  removeRecentFile(id: string): Promise<void>;
};

export type RecentFilesState = {
  replaceFiles(files: RecentFile[]): void;
  setLoading(loading: boolean): void;
};

export type RecentFileTrackingPort = {
  addRecentFileWithThumbnail(
    context: EditorCommandContext,
    originalPath?: string,
  ): Promise<RecentFile>;
};

export type RecentFileTrackingRequest = {
  originalPath?: string;
  context: EditorCommandContext;
};

type RecentFilesRuntime = {
  loadRequestId: number;
  activeLoadCount: number;
};

export type RecentFilesService = ReturnType<typeof createRecentFilesService>;
export type RecentFileTrackingService = ReturnType<typeof createRecentFileTrackingService>;

export function createRecentFilesService(
  store: RecentFilesState,
  port: RecentFilesPort,
) {
  const runtime: RecentFilesRuntime = {
    loadRequestId: 0,
    activeLoadCount: 0,
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

  async function remove(id: string) {
    await port.removeRecentFile(id);
    await load();
  }

  return { load, remove };
}

export function createRecentFileTrackingService(
  port: RecentFileTrackingPort,
  reportFailure: (error: unknown) => void,
) {
  async function tryAddRecentFileWithThumbnail({
    originalPath,
    context,
  }: RecentFileTrackingRequest): Promise<boolean> {
    try {
      await port.addRecentFileWithThumbnail(context, originalPath);
      return true;
    } catch (error) {
      reportFailure(error);
      return false;
    }
  }

  async function tryRefreshRecentFiles(refresh: () => Promise<void>): Promise<boolean> {
    try {
      await refresh();
      return true;
    } catch (error) {
      reportFailure(error);
      return false;
    }
  }

  return { tryAddRecentFileWithThumbnail, tryRefreshRecentFiles };
}
