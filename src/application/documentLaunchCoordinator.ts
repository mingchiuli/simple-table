import type { OpenTargetClaim } from '@/types/fileRuntime';

export type DocumentLaunchPort = {
  onLaunchTargetAvailable(handler: () => void): Promise<() => void>;
  claimPendingOpenTarget(): Promise<OpenTargetClaim | null>;
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
  let drainTail: Promise<void> = Promise.resolve();
  let registration: Promise<void> | null = null;
  let activeClaim: OpenTargetClaim | null = null;
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
    safeUnlisten(unlisten);
    unlisten = null;
    const pendingRegistration = registration;
    const pendingDrain = drainTail;
    disposal = Promise.allSettled([
      pendingRegistration ?? Promise.resolve(),
      pendingDrain,
    ]).then(() => undefined);
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
    drainTail = drainTail.then(
      () => drainPendingTargets(currentLifecycleId),
      () => drainPendingTargets(currentLifecycleId),
    );
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
        if (!handedOff) await releaseClaim(claim);
        if (activeClaim?.claimId === claim.claimId) activeClaim = null;
      }
      return;
    }
  }

  async function releaseClaim(claim: OpenTargetClaim) {
    try {
      await launchTargets.releaseOpenTarget(claim.claimId);
    } catch (error) {
      safeReportError('Failed to release document launch target:', error);
    }
  }

  function isCurrentLifecycle(currentLifecycleId: number) {
    return !disposed && lifecycleId === currentLifecycleId;
  }

  function safeUnlisten(value: (() => void) | null) {
    try {
      value?.();
    } catch (error) {
      safeReportError('Failed to clean up document launch listener:', error);
    }
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
