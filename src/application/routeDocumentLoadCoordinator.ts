import type { OperationCancellationSignal } from '@/application/operationCancellation';

export type RouteDocumentLoadPorts = {
  getRouteFilePath: () => string | null;
  getRouteOpenTargetClaimId: () => string | null;
  getCurrentFilePath: () => string | null;
  loadFileFromPath: (
    filePath: string,
    cancellation: OperationCancellationSignal,
  ) => Promise<boolean>;
  refreshEditorState: () => Promise<void>;
  acknowledgeOpenTarget: (claimId: string) => Promise<void>;
  releaseOpenTarget: (claimId: string) => Promise<void>;
  reportError: (error: unknown) => void;
};

export type RouteDocumentLoadCoordinator = {
  enqueue: (filePath: string | null, openTargetClaimId?: string | null) => void;
  cancel: () => void;
  waitForIdle: () => Promise<void>;
  dispose: () => Promise<void>;
};

type PendingRouteLoad = {
  filePath: string | null;
  openTargetClaimId: string | null;
  generation: number;
};

type ActiveCancellation = {
  generation: number;
  handlers: Set<() => void>;
};

export function createRouteDocumentLoadCoordinator({
  getRouteFilePath,
  getRouteOpenTargetClaimId,
  getCurrentFilePath,
  loadFileFromPath,
  refreshEditorState,
  acknowledgeOpenTarget,
  releaseOpenTarget,
  reportError,
}: RouteDocumentLoadPorts): RouteDocumentLoadCoordinator {
  let lastLoadedRouteFilePath: string | null = null;
  let routeLoadGeneration = 0;
  let pendingLoad: PendingRouteLoad | null = null;
  let workerPromise: Promise<void> | null = null;
  let activeCancellation: ActiveCancellation | null = null;
  const activeClaimSettlements = new Set<Promise<void>>();
  let disposed = false;
  let disposal: Promise<void> | null = null;

  function enqueue(filePath: string | null, openTargetClaimId: string | null = null) {
    if (disposed) {
      settleOpenTargetClaim(openTargetClaimId, 'release');
      return;
    }
    cancelActiveLoads();
    releaseSupersededPendingClaim(openTargetClaimId);
    const generation = ++routeLoadGeneration;
    pendingLoad = { filePath, openTargetClaimId, generation };
    startWorker();
  }

  function cancel() {
    cancelActiveLoads();
    routeLoadGeneration += 1;
    releaseSupersededPendingClaim(null);
    pendingLoad = null;
  }

  function dispose(): Promise<void> {
    if (disposal) return disposal;
    disposed = true;
    cancel();
    disposal = waitForIdle();
    return disposal;
  }

  function releaseSupersededPendingClaim(replacementClaimId: string | null) {
    const claimId = pendingLoad?.openTargetClaimId;
    if (claimId && claimId !== replacementClaimId) {
      settleOpenTargetClaim(claimId, 'release');
    }
  }

  function cancelActiveLoads() {
    const cancellation = activeCancellation;
    activeCancellation = null;
    for (const handler of cancellation?.handlers ?? []) {
      notifyCancellation(handler);
    }
  }

  function startWorker() {
    if (disposed || workerPromise) return;
    const worker = runWorker();
    workerPromise = worker;
    void worker.finally(() => {
      if (workerPromise === worker) workerPromise = null;
      if (!disposed && pendingLoad) startWorker();
    });
  }

  async function runWorker() {
    try {
      while (!disposed && pendingLoad) {
        const load = pendingLoad;
        pendingLoad = null;
        try {
          await runLoad(load);
        } catch (error) {
          safeReportError(error);
        }
      }
    } finally {
      if (disposed) releaseSupersededPendingClaim(null);
    }
  }

  async function runLoad({ filePath, openTargetClaimId, generation }: PendingRouteLoad) {
    if (!isCurrentRouteFileLoad(filePath, openTargetClaimId, generation)) {
      settleOpenTargetClaim(openTargetClaimId, 'release');
      return;
    }
    let claimOutcome: 'acknowledge' | 'release' = 'release';
    try {
      if (!filePath) {
        lastLoadedRouteFilePath = null;
        await refreshEditorState();
        return;
      }
      if (filePath === lastLoadedRouteFilePath && getCurrentFilePath() === filePath) {
        claimOutcome = 'acknowledge';
        return;
      }
      const cancellation = createCancellationSignal(filePath, openTargetClaimId, generation);
      try {
        if ((await loadFileFromPath(filePath, cancellation)) && !cancellation.isCancelled()) {
          lastLoadedRouteFilePath = filePath;
          claimOutcome = 'acknowledge';
        }
      } finally {
        if (activeCancellation?.generation === generation) {
          activeCancellation = null;
        }
      }
    } finally {
      settleOpenTargetClaim(openTargetClaimId, claimOutcome);
    }
  }

  function isCurrentRouteFileLoad(
    filePath: string | null,
    openTargetClaimId: string | null,
    generation: number,
  ) {
    return generation === routeLoadGeneration
      && filePath === getRouteFilePath()
      && openTargetClaimId === getRouteOpenTargetClaimId();
  }

  function createCancellationSignal(
    filePath: string,
    openTargetClaimId: string | null,
    generation: number,
  ): OperationCancellationSignal {
    const handlers = new Set<() => void>();
    activeCancellation = { generation, handlers };
    return {
      isCancelled: () => !isCurrentRouteFileLoad(filePath, openTargetClaimId, generation),
      onCancel(handler) {
        if (!isCurrentRouteFileLoad(filePath, openTargetClaimId, generation)) {
          notifyCancellation(handler);
          return () => undefined;
        }
        handlers.add(handler);
        return () => handlers.delete(handler);
      },
    };
  }

  function settleOpenTargetClaim(
    claimId: string | null,
    outcome: 'acknowledge' | 'release',
  ) {
    if (!claimId) return;
    const settlement = settleOpenTargetClaimWithRetry(claimId, outcome);
    activeClaimSettlements.add(settlement);
    void settlement.finally(() => activeClaimSettlements.delete(settlement));
  }

  async function settleOpenTargetClaimWithRetry(
    claimId: string,
    outcome: 'acknowledge' | 'release',
  ) {
    let lastError: unknown;
    for (let attempt = 0; attempt < 3; attempt += 1) {
      try {
        await (outcome === 'acknowledge'
          ? acknowledgeOpenTarget(claimId)
          : releaseOpenTarget(claimId));
        return;
      } catch (error) {
        lastError = error;
        if (attempt < 2) await Promise.resolve();
      }
    }
    safeReportError(lastError);
  }

  async function waitForIdle() {
    while (workerPromise || activeClaimSettlements.size > 0) {
      await Promise.allSettled([
        ...(workerPromise ? [workerPromise] : []),
        ...activeClaimSettlements,
      ]);
    }
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

  return { enqueue, cancel, waitForIdle, dispose };
}
