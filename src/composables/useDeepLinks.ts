import { listen } from '@tauri-apps/api/event';
import { isNavigationFailure, type Router } from 'vue-router';
import { onMounted, onUnmounted } from 'vue';

import {
  claimPendingOpenTarget,
  releaseOpenTarget,
} from '@/platform';
import type { OpenTargetClaim } from '@/types/fileRuntime';

type Unlisten = () => void;

type DeepLinkDependencies = {
  listen: (
    event: 'deep-link-received',
    handler: (event: { payload: unknown }) => void,
  ) => Promise<Unlisten>;
  claimPendingOpenTarget: () => Promise<OpenTargetClaim | null>;
  releaseOpenTarget: (claimId: string) => Promise<void>;
  pushFilePath: (filePath: string, claimId: string) => Promise<unknown>;
  reportError: (message: string, error: unknown) => void;
};

export type DeepLinkLifecycle = {
  start: () => void;
  stop: () => void;
};

export function useDeepLinks(router: Pick<Router, 'push'>) {
  const lifecycle = createDeepLinkLifecycle({
    listen,
    claimPendingOpenTarget,
    releaseOpenTarget,
    pushFilePath: async (filePath, claimId) => {
      const failure = await router.push({
        name: 'table',
        query: { file: filePath, openTargetClaim: claimId },
      });
      if (isNavigationFailure(failure)) throw failure;
    },
    reportError: (message, error) => console.error(message, error),
  });

  onMounted(lifecycle.start);
  onUnmounted(lifecycle.stop);
}

export function createDeepLinkLifecycle({
  listen,
  claimPendingOpenTarget,
  releaseOpenTarget,
  pushFilePath,
  reportError,
}: DeepLinkDependencies): DeepLinkLifecycle {
  let lifecycleId = 0;
  let unlisten: Unlisten | null = null;
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
      const registered = await listen('deep-link-received', () => {
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
        claim = await claimPendingOpenTarget();
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
        await pushFilePath(claim.path, claim.claimId);
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
      await releaseOpenTarget(claim.claimId);
    } catch (error) {
      reportError('Failed to release document launch target:', error);
    }
  }

  function isCurrentLifecycle(currentLifecycleId: number) {
    return lifecycleId === currentLifecycleId;
  }

  function safeUnlisten(value: Unlisten | null) {
    try {
      value?.();
    } catch (error) {
      reportError('Failed to clean up document launch listener:', error);
    }
  }

  return { start, stop };
}
