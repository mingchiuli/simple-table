type RouteFileLoaderOptions = {
  getRouteFilePath: () => string | null;
  getCurrentFilePath: () => string | null;
  loadFileFromPath: (
    filePath: string,
    shouldContinue: RouteContinuationGuard
  ) => Promise<boolean>;
  refreshEditorState: () => Promise<void>;
  reportError?: (error: unknown) => void;
};

type RouteContinuationGuard = (() => boolean) & {
  onCancel: (handler: () => void) => () => void;
};

type RouteFileLoaderCancellation = {
  cancel: () => void;
};

type PendingRouteLoad = {
  filePath: string | null;
  generation: number;
};

type RouteLeaveHandlerOptions = {
  routeFileLoader: RouteFileLoaderCancellation;
  hasActiveDocument: () => boolean;
  closeCurrentDocument: () => Promise<boolean>;
};

export function createRouteFileLoader({
  getRouteFilePath,
  getCurrentFilePath,
  loadFileFromPath,
  refreshEditorState,
  reportError = (error) => {
    console.error("Failed to handle route file load:", error);
  },
}: RouteFileLoaderOptions) {
  let lastLoadedRouteFilePath: string | null = null;
  let routeLoadGeneration = 0;
  let pendingLoad: PendingRouteLoad | null = null;
  let workerRunning = false;
  let activeCancellation: { generation: number; handlers: Set<() => void> } | null = null;

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
      handler();
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
          reportError(error);
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
    const guard = createContinuationGuard(filePath, generation);
    try {
      if ((await loadFileFromPath(filePath, guard)) && guard()) {
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

  function createContinuationGuard(
    filePath: string,
    generation: number
  ): RouteContinuationGuard {
    const guard = (() => isCurrentRouteFileLoad(filePath, generation)) as RouteContinuationGuard;
    const handlers = new Set<() => void>();
    activeCancellation = { generation, handlers };
    guard.onCancel = (handler) => {
      if (!guard()) {
        handler();
        return () => undefined;
      }
      handlers.add(handler);
      return () => {
        handlers.delete(handler);
      };
    };
    return guard;
  }

  return {
    enqueue,
    cancel,
  };
}

export function createRouteLeaveHandler({
  routeFileLoader,
  hasActiveDocument,
  closeCurrentDocument,
}: RouteLeaveHandlerOptions) {
  return async () => {
    if (!hasActiveDocument()) {
      routeFileLoader.cancel();
      return true;
    }

    const canLeave = await closeCurrentDocument();
    if (canLeave) {
      routeFileLoader.cancel();
    }
    return canLeave;
  };
}
