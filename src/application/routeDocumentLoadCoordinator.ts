import {
  createOperationCancellationSource,
  type OperationCancellationSignal,
} from '@/application/operationCancellation';

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

type ClaimOutcome = 'acknowledge' | 'superseded' | 'transferred';

type ActiveCancellation = {
  generation: number;
  openTargetClaimId: string | null;
  claimOutcome: ClaimOutcome | null;
  source: ReturnType<typeof createOperationCancellationSource>;
};

export function createRouteDocumentLoadCoordinator({
  getRouteFilePath,
  getRouteOpenTargetClaimId,
  getCurrentFilePath,
  loadFileFromPath,
  refreshEditorState,
  acknowledgeOpenTarget,
  reportError,
}: RouteDocumentLoadPorts): RouteDocumentLoadCoordinator {
  let lastLoadedRouteFilePath: string | null = null;
  let routeLoadGeneration = 0;
  let pendingLoad: PendingRouteLoad | null = null;
  let workerPromise: Promise<void> | null = null;
  let activeCancellation: ActiveCancellation | null = null;
  const activeClaimSettlements = new Set<Promise<void>>();
  const claimSettlementFailures: unknown[] = [];
  const cancellationNotificationFailures: unknown[] = [];
  let disposed = false;
  let disposal: Promise<void> | null = null;

  function enqueue(filePath: string | null, openTargetClaimId: string | null = null) {
    if (disposed) {
      settleOpenTargetClaim(openTargetClaimId, 'acknowledge');
      return;
    }
    cancelActiveLoads('superseded', openTargetClaimId);
    settleReplacedPendingClaim(openTargetClaimId, 'superseded');
    const generation = ++routeLoadGeneration;
    pendingLoad = { filePath, openTargetClaimId, generation };
    startWorker();
  }

  function cancel() {
    cancelActiveLoads('superseded');
    routeLoadGeneration += 1;
    settleReplacedPendingClaim(null, 'superseded');
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
    outcome: Exclude<ClaimOutcome, 'transferred'>,
  ) {
    const claimId = pendingLoad?.openTargetClaimId;
    if (claimId && claimId !== replacementClaimId) {
      settleOpenTargetClaim(claimId, outcome);
    }
  }

  function cancelActiveLoads(
    outcome: Exclude<ClaimOutcome, 'transferred'>,
    replacementClaimId: string | null = null,
  ) {
    const cancellation = activeCancellation;
    activeCancellation = null;
    if (!cancellation) return;
    cancellation.claimOutcome = cancellation.openTargetClaimId
      && cancellation.openTargetClaimId === replacementClaimId
      ? 'transferred'
      : outcome;
    for (const error of cancellation.source.cancel()) {
      recordCancellationNotificationFailure(error);
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
      if (disposed) settleReplacedPendingClaim(null, 'superseded');
    }
  }

  async function runLoad({ filePath, openTargetClaimId, generation }: PendingRouteLoad) {
    if (!isCurrentRouteFileLoad(filePath, openTargetClaimId, generation)) {
      settleOpenTargetClaim(openTargetClaimId, 'acknowledge');
      return;
    }
    let claimOutcome: ClaimOutcome = 'acknowledge';
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
    const source = createOperationCancellationSource();
    const state: ActiveCancellation = {
      generation,
      openTargetClaimId,
      claimOutcome: null,
      source,
    };
    activeCancellation = state;
    const signal: OperationCancellationSignal = {
      isCancelled: () => source.signal.isCancelled()
        || !isCurrentRouteFileLoad(filePath, openTargetClaimId, generation),
      onCancel(handler) {
        if (signal.isCancelled()) {
          try {
            handler();
          } catch (error) {
            recordCancellationNotificationFailure(error);
          }
          return () => undefined;
        }
        return source.signal.onCancel(handler);
      },
    };
    return { signal, state };
  }

  function settleOpenTargetClaim(
    claimId: string | null,
    outcome: ClaimOutcome,
  ) {
    if (!claimId || outcome === 'transferred') return;
    const settlement = settleOpenTargetClaimWithRetry(claimId);
    let tracked!: Promise<void>;
    tracked = settlement.then(
      () => undefined,
      (error) => {
        claimSettlementFailures.push(error);
      },
    ).then(() => {
      activeClaimSettlements.delete(tracked);
    });
    activeClaimSettlements.add(tracked);
  }

  async function settleOpenTargetClaimWithRetry(claimId: string) {
    let lastError: unknown;
    for (let attempt = 0; attempt < 3; attempt += 1) {
      try {
        // A superseded launch intent is terminal: consume it instead of requeueing stale work.
        await acknowledgeOpenTarget(claimId);
        return;
      } catch (error) {
        lastError = error;
        if (attempt < 2) await Promise.resolve();
      }
    }
    safeReportError(lastError);
    throw lastError;
  }

  function recordCancellationNotificationFailure(error: unknown) {
    cancellationNotificationFailures.push(error);
    safeReportError(error);
  }

  async function waitForIdle() {
    while (workerPromise || activeClaimSettlements.size > 0) {
      await Promise.allSettled([
        ...(workerPromise ? [workerPromise] : []),
        ...activeClaimSettlements,
      ]);
    }
    const failures = [
      ...cancellationNotificationFailures,
      ...claimSettlementFailures,
    ];
    if (failures.length > 0) {
      throw new AggregateError(
        failures,
        'Failed to completely drain route document loading',
      );
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
