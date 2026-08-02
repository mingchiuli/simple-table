import type { OpenTargetClaim } from '@/types/fileRuntime';
import { createResilientEventSubscription } from '@/application/resilientEventSubscription';

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

type DocumentLaunchCoordinatorOptions = {
  waitBeforeListenerRetry?: () => Promise<void>;
  listenerRegistrationTimeoutMs?: number;
};

export function createDocumentLaunchCoordinator({
  launchTargets,
  openTarget,
  reportError,
}: DocumentLaunchCoordinatorPorts, {
  waitBeforeListenerRetry,
  listenerRegistrationTimeoutMs,
}: DocumentLaunchCoordinatorOptions = {}): DocumentLaunchCoordinator {
  let lifecycleId = 0;
  let drainRequested = false;
  let drainWorker: Promise<void> | null = null;
  let activeClaim: OpenTargetClaim | null = null;
  const cleanupFailures: unknown[] = [];
  let started = false;
  let disposed = false;
  let disposal: Promise<void> | null = null;
  const listener = createResilientEventSubscription({
    subscribe: (handler) => launchTargets.onLaunchTargetAvailable(handler),
    handler: () => requestDrain(lifecycleId),
    reportError: safeReportError,
    registrationErrorMessage: 'Failed to initialize document launch listener:',
    cleanupErrorMessage: 'Failed to clean up document launch listener:',
    onSubscribed: () => requestDrain(lifecycleId),
    waitBeforeRetry: waitBeforeListenerRetry,
    registrationTimeoutMs: listenerRegistrationTimeoutMs,
  });

  function start() {
    if (started || disposed) return;
    started = true;
    const currentLifecycleId = ++lifecycleId;
    requestDrain(currentLifecycleId);
    void listener.start();
  }

  function dispose(): Promise<void> {
    if (disposal) return disposal;
    disposed = true;
    lifecycleId += 1;
    drainRequested = false;
    listener.dispose();
    const pendingDrain = drainWorker;
    disposal = Promise.allSettled([
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
