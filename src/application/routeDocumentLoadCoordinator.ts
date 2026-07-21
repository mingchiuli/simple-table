import type { OperationCancellationSignal } from '@/application/operationCancellation';

export type RouteDocumentLoadPorts = {
  getRouteFilePath: () => string | null;
  getCurrentFilePath: () => string | null;
  loadFileFromPath: (
    filePath: string,
    cancellation: OperationCancellationSignal,
  ) => Promise<boolean>;
  refreshEditorState: () => Promise<void>;
  reportError: (error: unknown) => void;
};

export type RouteDocumentLoadCoordinator = {
  enqueue: (filePath: string | null) => void;
  cancel: () => void;
};

type PendingRouteLoad = {
  filePath: string | null;
  generation: number;
};

type ActiveCancellation = {
  generation: number;
  handlers: Set<() => void>;
};

export function createRouteDocumentLoadCoordinator({
  getRouteFilePath,
  getCurrentFilePath,
  loadFileFromPath,
  refreshEditorState,
  reportError,
}: RouteDocumentLoadPorts): RouteDocumentLoadCoordinator {
  let lastLoadedRouteFilePath: string | null = null;
  let routeLoadGeneration = 0;
  let pendingLoad: PendingRouteLoad | null = null;
  let workerRunning = false;
  let activeCancellation: ActiveCancellation | null = null;

  function enqueue(filePath: string | null) {
    cancelActiveLoads();
    const generation = ++routeLoadGeneration;
    pendingLoad = { filePath, generation };
    void runWorker();
  }

  function cancel() {
    cancelActiveLoads();
    routeLoadGeneration += 1;
    pendingLoad = null;
  }

  function cancelActiveLoads() {
    const cancellation = activeCancellation;
    activeCancellation = null;
    for (const handler of cancellation?.handlers ?? []) {
      notifyCancellation(handler);
    }
  }

  async function runWorker() {
    if (workerRunning) return;
    workerRunning = true;
    try {
      while (pendingLoad) {
        const load = pendingLoad;
        pendingLoad = null;
        try {
          await runLoad(load);
        } catch (error) {
          safeReportError(error);
        }
      }
    } finally {
      workerRunning = false;
      if (pendingLoad) void runWorker();
    }
  }

  async function runLoad({ filePath, generation }: PendingRouteLoad) {
    if (!isCurrentRouteFileLoad(filePath, generation)) return;
    if (!filePath) {
      lastLoadedRouteFilePath = null;
      await refreshEditorState();
      return;
    }
    if (filePath === lastLoadedRouteFilePath && getCurrentFilePath() === filePath) {
      return;
    }
    const cancellation = createCancellationSignal(filePath, generation);
    try {
      if ((await loadFileFromPath(filePath, cancellation)) && !cancellation.isCancelled()) {
        lastLoadedRouteFilePath = filePath;
      }
    } finally {
      if (activeCancellation?.generation === generation) {
        activeCancellation = null;
      }
    }
  }

  function isCurrentRouteFileLoad(filePath: string | null, generation: number) {
    return generation === routeLoadGeneration && filePath === getRouteFilePath();
  }

  function createCancellationSignal(
    filePath: string,
    generation: number,
  ): OperationCancellationSignal {
    const handlers = new Set<() => void>();
    activeCancellation = { generation, handlers };
    return {
      isCancelled: () => !isCurrentRouteFileLoad(filePath, generation),
      onCancel(handler) {
        if (!isCurrentRouteFileLoad(filePath, generation)) {
          notifyCancellation(handler);
          return () => undefined;
        }
        handlers.add(handler);
        return () => handlers.delete(handler);
      },
    };
  }

  function notifyCancellation(handler: () => void) {
    try {
      handler();
    } catch (error) {
      safeReportError(error);
    }
  }

  function safeReportError(error: unknown) {
    try {
      reportError(error);
    } catch {
      // Error reporting must not terminate the route-load worker.
    }
  }

  return { enqueue, cancel };
}
