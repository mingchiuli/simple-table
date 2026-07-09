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

export function createRouteFileLoader({
  getRouteFilePath,
  getCurrentFilePath,
  loadFileFromPath,
  refreshEditorState,
  reportError = (error) => {
    console.error("Failed to handle route file load:", error);
  },
}: RouteFileLoaderOptions) {
  let routeLoadQueue = Promise.resolve();
  let lastLoadedRouteFilePath: string | null = null;
  let routeLoadGeneration = 0;
  const cancelHandlersByGeneration = new Map<number, Set<() => void>>();

  function enqueue(filePath: string | null) {
    cancelActiveLoads();
    const generation = ++routeLoadGeneration;
    routeLoadQueue = routeLoadQueue
      .catch(() => undefined)
      .then(async () => {
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
          if (await loadFileFromPath(filePath, guard)) {
            lastLoadedRouteFilePath = filePath;
          }
        } finally {
          cancelHandlersByGeneration.delete(generation);
        }
      })
      .catch(reportError);
  }

  function cancel() {
    cancelActiveLoads();
    routeLoadGeneration += 1;
  }

  function cancelActiveLoads() {
    const generations = Array.from(cancelHandlersByGeneration.keys());
    for (const generation of generations) {
      const handlers = cancelHandlersByGeneration.get(generation);
      cancelHandlersByGeneration.delete(generation);
      for (const handler of handlers ?? []) {
        handler();
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
    cancelHandlersByGeneration.set(generation, handlers);
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
