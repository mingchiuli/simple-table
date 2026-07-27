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

type ClaimOutcome = 'acknowledge' | 'release' | 'superseded' | 'transferred';
type ClaimInterruptionOutcome = Extract<ClaimOutcome, 'release' | 'superseded'>;

type ActiveCancellation = {
  generation: number;
  openTargetClaimId: string | null;
  cancelled: boolean;
  claimOutcome: ClaimOutcome | null;
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
    cancelActiveLoads('superseded', openTargetClaimId);
    settleReplacedPendingClaim(openTargetClaimId, 'superseded');
    const generation = ++routeLoadGeneration;
    pendingLoad = { filePath, openTargetClaimId, generation };
    startWorker();
  }

  function cancel() {
    cancelActiveLoads('release');
    routeLoadGeneration += 1;
    settleReplacedPendingClaim(null, 'release');
    pendingLoad = null;
  }

  function dispose(): Promise<void> {
    if (disposal) return disposal;
    disposed = true;
    cancel();
    disposal = waitForIdle();
    return disposal;
  }

  function settleReplacedPendingClaim(
    replacementClaimId: string | null,
    outcome: ClaimInterruptionOutcome,
  ) {
    const claimId = pendingLoad?.openTargetClaimId;
    if (claimId && claimId !== replacementClaimId) {
      settleOpenTargetClaim(claimId, outcome);
    }
  }

  function cancelActiveLoads(
    outcome: ClaimInterruptionOutcome,
    replacementClaimId: string | null = null,
  ) {
    const cancellation = activeCancellation;
    activeCancellation = null;
    if (!cancellation) return;
    cancellation.cancelled = true;
    cancellation.claimOutcome = cancellation.openTargetClaimId
      && cancellation.openTargetClaimId === replacementClaimId
      ? 'transferred'
      : outcome;
    for (const handler of cancellation.handlers) {
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
      if (disposed) settleReplacedPendingClaim(null, 'release');
    }
  }

  async function runLoad({ filePath, openTargetClaimId, generation }: PendingRouteLoad) {
    if (!isCurrentRouteFileLoad(filePath, openTargetClaimId, generation)) {
      settleOpenTargetClaim(openTargetClaimId, 'release');
      return;
    }
    let claimOutcome: ClaimOutcome = 'release';
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
      const cancellation = createActiveCancellation(filePath, openTargetClaimId, generation);
      try {
        if ((await loadFileFromPath(filePath, cancellation.signal))
          && !cancellation.signal.isCancelled()) {
          lastLoadedRouteFilePath = filePath;
          claimOutcome = 'acknowledge';
        }
      } finally {
        if (cancellation.state.claimOutcome) {
          claimOutcome = cancellation.state.claimOutcome;
        }
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

  function createActiveCancellation(
    filePath: string,
    openTargetClaimId: string | null,
    generation: number,
  ): { signal: OperationCancellationSignal; state: ActiveCancellation } {
    const handlers = new Set<() => void>();
    const state: ActiveCancellation = {
      generation,
      openTargetClaimId,
      cancelled: false,
      claimOutcome: null,
      handlers,
    };
    activeCancellation = state;
    const signal: OperationCancellationSignal = {
      isCancelled: () => state.cancelled
        || !isCurrentRouteFileLoad(filePath, openTargetClaimId, generation),
      onCancel(handler) {
        if (signal.isCancelled()) {
          notifyCancellation(handler);
          return () => undefined;
        }
        handlers.add(handler);
        return () => handlers.delete(handler);
      },
    };
    return { signal, state };
  }

  function settleOpenTargetClaim(
    claimId: string | null,
    outcome: ClaimOutcome,
  ) {
    if (!claimId || outcome === 'transferred') return;
    const settlement = settleOpenTargetClaimWithRetry(claimId, outcome);
    activeClaimSettlements.add(settlement);
    void settlement.finally(() => activeClaimSettlements.delete(settlement));
  }

  async function settleOpenTargetClaimWithRetry(
    claimId: string,
    outcome: Exclude<ClaimOutcome, 'transferred'>,
  ) {
    let lastError: unknown;
    for (let attempt = 0; attempt < 3; attempt += 1) {
      try {
        // A superseded launch intent is terminal: consume it instead of requeueing stale work.
        await (outcome === 'release'
          ? releaseOpenTarget(claimId)
          : acknowledgeOpenTarget(claimId));
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
