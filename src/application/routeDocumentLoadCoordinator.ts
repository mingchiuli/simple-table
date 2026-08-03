import {
  createOperationCancellationSource,
  isOperationCancelled,
  type OperationCancellationSignal,
} from '@/application/operationCancellation';
import { runBeforeDeadline } from '@/application/operationOutcome';

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
  renewOpenTarget: (claimId: string) => Promise<boolean>;
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

type RouteDocumentLoadCoordinatorOptions = {
  claimRenewIntervalMs?: number;
  claimRenewTimeoutMs?: number;
};

const DEFAULT_CLAIM_RENEW_INTERVAL_MS = 15_000;
const DEFAULT_CLAIM_RENEW_TIMEOUT_MS = 5_000;

class OpenTargetClaimLostError extends Error {
  constructor(claimId: string) {
    super(`Open target claim ${claimId} expired or is no longer owned by this loader`);
    this.name = 'OpenTargetClaimLostError';
  }
}

class OpenTargetClaimRenewalTimeoutError extends Error {
  constructor(claimId: string, timeoutMs: number) {
    super(`Open target claim ${claimId} renewal timed out after ${timeoutMs} ms`);
    this.name = 'OpenTargetClaimRenewalTimeoutError';
  }
}

export function createRouteDocumentLoadCoordinator({
  getRouteFilePath,
  getRouteOpenTargetClaimId,
  getCurrentFilePath,
  loadFileFromPath,
  refreshEditorState,
  acknowledgeOpenTarget,
  renewOpenTarget,
  reportError,
}: RouteDocumentLoadPorts, {
  claimRenewIntervalMs = DEFAULT_CLAIM_RENEW_INTERVAL_MS,
  claimRenewTimeoutMs = DEFAULT_CLAIM_RENEW_TIMEOUT_MS,
}: RouteDocumentLoadCoordinatorOptions = {}): RouteDocumentLoadCoordinator {
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
      const claimLease = openTargetClaimId
        ? startOpenTargetClaimLease(openTargetClaimId, cancellation.state)
        : null;
      try {
        if ((await loadFileFromPath(filePath, cancellation.signal))
          && !cancellation.signal.isCancelled()) {
          lastLoadedRouteFilePath = filePath;
          claimOutcome = 'acknowledge';
        }
      } catch (error) {
        if (!(cancellation.state.claimOutcome === 'transferred'
          && isOperationCancelled(error))) {
          throw error;
        }
      } finally {
        await claimLease?.stop();
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

  function startOpenTargetClaimLease(claimId: string, state: ActiveCancellation) {
    const leaseCancellation = createOperationCancellationSource();
    const renewal = renewClaimUntilStopped(claimId, state, leaseCancellation.signal);
    return {
      async stop() {
        for (const error of leaseCancellation.cancel()) {
          recordCancellationNotificationFailure(error);
        }
        await renewal;
      },
    };
  }

  async function renewClaimUntilStopped(
    claimId: string,
    state: ActiveCancellation,
    cancellation: OperationCancellationSignal,
  ) {
    while (!cancellation.isCancelled()) {
      await waitForClaimRenewal(cancellation, claimRenewIntervalMs);
      if (cancellation.isCancelled()) return;
      let renewed: boolean;
      try {
        renewed = await runBeforeDeadline(
          () => renewOpenTarget(claimId),
          claimRenewTimeoutMs,
          () => new OpenTargetClaimRenewalTimeoutError(claimId, claimRenewTimeoutMs),
          cancellation,
        );
      } catch (error) {
        if (cancellation.isCancelled() && isOperationCancelled(error)) return;
        safeReportError(error);
        continue;
      }
      if (renewed) continue;

      state.claimOutcome = 'transferred';
      for (const error of state.source.cancel()) {
        recordCancellationNotificationFailure(error);
      }
      safeReportError(new OpenTargetClaimLostError(claimId));
      return;
    }
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

function waitForClaimRenewal(
  cancellation: OperationCancellationSignal,
  delayMs: number,
): Promise<void> {
  if (cancellation.isCancelled()) return Promise.resolve();
  return new Promise((resolve) => {
    let settled = false;
    let unregister: () => void = () => undefined;
    const settle = () => {
      if (settled) return;
      settled = true;
      unregister();
      resolve();
    };
    const timeout = setTimeout(settle, delayMs);
    unregister = cancellation.onCancel(() => {
      clearTimeout(timeout);
      settle();
    });
  });
}
