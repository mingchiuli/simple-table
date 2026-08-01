import type { FileOperationReceipt } from '@/types/fileRuntime';
import type { RecentFile } from '@/types/recentFileRuntime';
import { createWorkspaceOperationTracker } from '@/application/workspaceOperationTracker';
import {
  createOperationCancellationSource,
  isOperationCancelled,
  neverCancelled,
  raceWithOperationCancellation,
  throwIfOperationCancellationFailed,
  type OperationCancellationSignal,
} from '@/application/operationCancellation';
import { drainAllSettled } from '@/application/asyncDrain';

export type RecentFilesPort = {
  getRecentFiles(): Promise<RecentFile[]>;
  removeRecentFile(id: string): Promise<void>;
  addRecentFileWithThumbnail(
    receipt: FileOperationReceipt,
    originalPath?: string,
  ): Promise<void>;
};

export type RecentFilesState = {
  replaceFiles(files: RecentFile[]): void;
  setLoading(loading: boolean): void;
};

export type RecentFileTrackingRequest = {
  originalPath?: string;
  receipt: FileOperationReceipt;
};

type RecentFilesRuntime = {
  loadRequestId: number;
  activeLoadCount: number;
  activeTracking: Promise<void> | null;
  pendingTracking: Map<string, RecentFileTrackingRequest>;
};

const MAX_PENDING_RECENT_FILE_UPDATES = 1_024;

export type RecentFilesService = ReturnType<typeof createRecentFilesService>;

export function createRecentFilesService(
  store: RecentFilesState,
  port: RecentFilesPort,
  reportFailure: (error: unknown) => void = () => undefined,
  parentCancellation: OperationCancellationSignal = neverCancelled,
) {
  const runtime: RecentFilesRuntime = {
    loadRequestId: 0,
    activeLoadCount: 0,
    activeTracking: null,
    pendingTracking: new Map(),
  };
  const operations = createWorkspaceOperationTracker();
  const observationCancellation = createOperationCancellationSource();
  const unlinkParentCancellation = parentCancellation.onCancel(observationCancellation.cancel);
  let disposed = false;
  let disposal: Promise<void> | null = null;

  function load(): Promise<void> {
    return operations.run(runLoad, undefined);
  }

  async function runLoad() {
    const requestId = runtime.loadRequestId + 1;
    runtime.loadRequestId = requestId;
    runtime.activeLoadCount += 1;
    store.setLoading(true);
    try {
      const files = await raceWithOperationCancellation(
        () => port.getRecentFiles(),
        observationCancellation.signal,
      );
      if (!disposed && requestId === runtime.loadRequestId) {
        store.replaceFiles(files);
      }
    } finally {
      runtime.activeLoadCount = Math.max(0, runtime.activeLoadCount - 1);
      if (!disposed) store.setLoading(runtime.activeLoadCount > 0);
    }
  }

  async function refresh(): Promise<boolean> {
    try {
      await load();
      return true;
    } catch (error) {
      if (isOperationCancelled(error)) return false;
      safeReportFailure(error);
      return false;
    }
  }

  function remove(id: string): Promise<void> {
    return operations.run(async () => {
      await port.removeRecentFile(id);
      if (operations.isAcceptingWork()) await runLoad();
    }, undefined);
  }

  function queueRecentFileEntryUpdate(request: RecentFileTrackingRequest) {
    if (!operations.isAcceptingWork()) return;
    const key = request.receipt.path;
    runtime.pendingTracking.delete(key);
    runtime.pendingTracking.set(key, request);
    while (runtime.pendingTracking.size > MAX_PENDING_RECENT_FILE_UPDATES) {
      const oldestKey = runtime.pendingTracking.keys().next().value;
      if (oldestKey === undefined) break;
      runtime.pendingTracking.delete(oldestKey);
    }
    startTrackingWorker();
  }

  function startTrackingWorker() {
    if (runtime.activeTracking) return;
    const worker = operations.runRequired(runTrackingWorker).catch(safeReportFailure);
    runtime.activeTracking = worker;
    void worker.then(() => {
      if (runtime.activeTracking === worker) runtime.activeTracking = null;
      if (!disposed && runtime.pendingTracking.size > 0) startTrackingWorker();
    });
  }

  async function runTrackingWorker() {
    while (!disposed) {
      while (!disposed && runtime.pendingTracking.size > 0) {
        const pending = runtime.pendingTracking.entries().next().value;
        if (!pending) break;
        const [key, request] = pending;
        runtime.pendingTracking.delete(key);
        try {
          await port.addRecentFileWithThumbnail(request.receipt, request.originalPath);
        } catch (error) {
          safeReportFailure(error);
        }
      }
      if (disposed) return;
      await refresh();
      if (disposed || runtime.pendingTracking.size === 0) return;
    }
  }

  function dispose(): Promise<void> {
    if (disposal) return disposal;
    disposed = true;
    operations.stopAcceptingWork();
    const cancellationFailures = observationCancellation.cancel();
    unlinkParentCancellation();
    runtime.loadRequestId += 1;
    runtime.pendingTracking.clear();
    store.setLoading(false);
    disposal = drainAllSettled([
      () => throwIfOperationCancellationFailed(
        cancellationFailures,
        'Failed to notify every recent-files cancellation observer',
      ),
      waitForIdle,
    ], 'Failed to completely drain recent-file coordination');
    return disposal;
  }

  async function waitForIdle(): Promise<void> {
    while (runtime.activeTracking) {
      const pending = [
        ...(runtime.activeTracking ? [runtime.activeTracking] : []),
      ];
      await Promise.allSettled(pending);
    }
    await operations.waitForIdle();
    operations.markDisposed();
  }

  function safeReportFailure(error: unknown) {
    try {
      reportFailure(error);
    } catch {
      // Failure reporting must not reject a background metadata worker.
    }
  }

  return { load, refresh, remove, queueRecentFileEntryUpdate, waitForIdle, dispose };
}
