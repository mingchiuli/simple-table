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
  let workerRunning = false;
  let activeCancellation: ActiveCancellation | null = null;
  const activeClaimSettlements = new Set<Promise<void>>();

  function enqueue(filePath: string | null, openTargetClaimId: string | null = null) {
    cancelActiveLoads();
    releaseSupersededPendingClaim(openTargetClaimId);
    const generation = ++routeLoadGeneration;
    pendingLoad = { filePath, openTargetClaimId, generation };
    void runWorker();
  }

  function cancel() {
    cancelActiveLoads();
    routeLoadGeneration += 1;
    releaseSupersededPendingClaim(null);
    pendingLoad = null;
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
    const settlement = (outcome === 'acknowledge'
      ? acknowledgeOpenTarget(claimId)
      : releaseOpenTarget(claimId))
      .catch(safeReportError);
    activeClaimSettlements.add(settlement);
    void settlement.finally(() => activeClaimSettlements.delete(settlement));
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
