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
  stop(): void;
};

export function createDocumentLaunchCoordinator({
  launchTargets,
  openTarget,
  reportError,
}: DocumentLaunchCoordinatorPorts): DocumentLaunchCoordinator {
  let lifecycleId = 0;
  let unlisten: (() => void) | null = null;
  let drainTail: Promise<void> = Promise.resolve();

  function start() {
    stop();
    const currentLifecycleId = ++lifecycleId;
    void registerListener(currentLifecycleId);
  }

  function stop() {
    lifecycleId += 1;
    safeUnlisten(unlisten);
    unlisten = null;
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
      reportError('Failed to initialize document launch listener:', error);
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
          reportError('Failed to claim pending document launch target:', error);
        }
        return;
      }
      if (!claim) return;
      if (!isCurrentLifecycle(currentLifecycleId)) {
        await releaseClaim(claim);
        return;
      }
      try {
        await openTarget(claim.path, claim.claimId);
      } catch (error) {
        reportError('Failed to route document launch target:', error);
        await releaseClaim(claim);
        return;
      }
      return;
    }
  }

  async function releaseClaim(claim: OpenTargetClaim) {
    try {
      await launchTargets.releaseOpenTarget(claim.claimId);
    } catch (error) {
      reportError('Failed to release document launch target:', error);
    }
  }

  function isCurrentLifecycle(currentLifecycleId: number) {
    return lifecycleId === currentLifecycleId;
  }

  function safeUnlisten(value: (() => void) | null) {
    try {
      value?.();
    } catch (error) {
      reportError('Failed to clean up document launch listener:', error);
    }
  }

  return { start, stop };
}
