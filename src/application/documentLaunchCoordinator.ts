import type { OpenTargetClaim } from '@/types/fileRuntime';

export type DocumentLaunchPort = {
  onLaunchTargetAvailable(handler: () => void): Promise<() => void>;
  claimPendingOpenTarget(): Promise<OpenTargetClaim | null>;
  acknowledgeOpenTarget(claimId: string): Promise<void>;
  releaseOpenTarget(claimId: string): Promise<void>;
};

export type DocumentLaunchCoordinatorPorts = {
  launchTargets: DocumentLaunchPort;
  openTarget(filePath: string, claimId: string): Promise<void>;
  reportError(message: string, error: unknown): void;
};

export type DocumentLaunchCoordinator = {
  start(): void;
  dispose(): Promise<void>;
};

export function createDocumentLaunchCoordinator({
  launchTargets,
  openTarget,
  reportError,
}: DocumentLaunchCoordinatorPorts): DocumentLaunchCoordinator {
  let lifecycleId = 0;
  let unlisten: (() => void) | null = null;
  let drainRequested = false;
  let drainWorker: Promise<void> | null = null;
  let registration: Promise<void> | null = null;
  let activeClaim: OpenTargetClaim | null = null;
  const cleanupFailures: unknown[] = [];
  let started = false;
  let disposed = false;
  let disposal: Promise<void> | null = null;

  function start() {
    if (started || disposed) return;
    started = true;
    const currentLifecycleId = ++lifecycleId;
    registration = registerListener(currentLifecycleId);
  }

  function dispose(): Promise<void> {
    if (disposal) return disposal;
    disposed = true;
    lifecycleId += 1;
    drainRequested = false;
    safeUnlisten(unlisten);
    unlisten = null;
    const pendingRegistration = registration;
    const pendingDrain = drainWorker;
    disposal = Promise.allSettled([
      pendingRegistration ?? Promise.resolve(),
      pendingDrain,
    ]).then(() => {
      if (cleanupFailures.length > 0) {
        throw new AggregateError(
          cleanupFailures,
          'Failed to completely dispose document launch coordination',
        );
      }
    });
    return disposal;
  }

  async function registerListener(currentLifecycleId: number) {
    try {
      const registered = await launchTargets.onLaunchTargetAvailable(() => {
        requestDrain(currentLifecycleId);
      });
      if (!isCurrentLifecycle(currentLifecycleId)) {
        safeUnlisten(registered);
        return;
      }
      unlisten = registered;
      requestDrain(currentLifecycleId);
    } catch (error) {
      safeReportError('Failed to initialize document launch listener:', error);
    }
  }

  function requestDrain(currentLifecycleId: number) {
    if (!isCurrentLifecycle(currentLifecycleId)) return;
    drainRequested = true;
    if (drainWorker) return;
    const worker = runDrainWorker(currentLifecycleId);
    drainWorker = worker;
    void worker.finally(() => {
      if (drainWorker === worker) drainWorker = null;
      if (drainRequested && isCurrentLifecycle(currentLifecycleId)) {
        requestDrain(currentLifecycleId);
      }
    });
  }

  async function runDrainWorker(currentLifecycleId: number) {
    while (drainRequested && isCurrentLifecycle(currentLifecycleId)) {
      drainRequested = false;
      await drainPendingTargets(currentLifecycleId);
    }
  }

  async function drainPendingTargets(currentLifecycleId: number) {
    if (!isCurrentLifecycle(currentLifecycleId)) return;
    while (isCurrentLifecycle(currentLifecycleId)) {
      let claim: OpenTargetClaim | null;
      try {
        claim = await launchTargets.claimPendingOpenTarget();
      } catch (error) {
        if (isCurrentLifecycle(currentLifecycleId)) {
          safeReportError('Failed to claim pending document launch target:', error);
        }
        return;
      }
      if (!claim) return;
      if (!isCurrentLifecycle(currentLifecycleId)) {
        await releaseClaim(claim);
        return;
      }
      activeClaim = claim;
      let handedOff = false;
      try {
        await openTarget(claim.path, claim.claimId);
        handedOff = true;
      } catch (error) {
        safeReportError('Failed to route document launch target:', error);
      } finally {
        if (!handedOff) {
          await (isCurrentLifecycle(currentLifecycleId)
            ? acknowledgeClaim(claim)
            : releaseClaim(claim));
        }
        if (activeClaim?.claimId === claim.claimId) activeClaim = null;
      }
      return;
    }
  }

  async function releaseClaim(claim: OpenTargetClaim) {
    try {
      await launchTargets.releaseOpenTarget(claim.claimId);
    } catch (error) {
      recordCleanupFailure('Failed to release document launch target:', error);
    }
  }

  async function acknowledgeClaim(claim: OpenTargetClaim) {
    try {
      await launchTargets.acknowledgeOpenTarget(claim.claimId);
    } catch (error) {
      recordCleanupFailure('Failed to acknowledge document launch target:', error);
    }
  }

  function isCurrentLifecycle(currentLifecycleId: number) {
    return !disposed && lifecycleId === currentLifecycleId;
  }

  function safeUnlisten(value: (() => void) | null) {
    try {
      value?.();
    } catch (error) {
      recordCleanupFailure('Failed to clean up document launch listener:', error);
    }
  }

  function recordCleanupFailure(message: string, error: unknown) {
    cleanupFailures.push(error);
    safeReportError(message, error);
  }

  function safeReportError(message: string, error: unknown) {
    try {
      reportError(message, error);
    } catch {
      // Error reporting must not interrupt claim settlement or listener disposal.
    }
  }

  return { start, dispose };
}
