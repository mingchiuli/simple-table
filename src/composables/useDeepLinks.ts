import { listen } from '@tauri-apps/api/event';
import type { Router } from 'vue-router';
import { onMounted, onUnmounted } from 'vue';

import { takePendingOpenTargets } from '@/platform';

type Unlisten = () => void;

type DeepLinkDependencies = {
  listen: (
    event: 'deep-link-received',
    handler: (event: { payload: unknown }) => void,
  ) => Promise<Unlisten>;
  takePendingOpenTargets: () => Promise<string[]>;
  pushFilePath: (filePath: string) => Promise<unknown>;
  reportError: (message: string, error: unknown) => void;
};

export type DeepLinkLifecycle = {
  start: () => void;
  stop: () => void;
};

export function useDeepLinks(router: Pick<Router, 'push'>) {
  const lifecycle = createDeepLinkLifecycle({
    listen,
    takePendingOpenTargets,
    pushFilePath: (filePath) => router.push({ name: 'table', query: { file: filePath } }),
    reportError: (message, error) => console.error(message, error),
  });

  onMounted(lifecycle.start);
  onUnmounted(lifecycle.stop);
}

export function createDeepLinkLifecycle({
  listen,
  takePendingOpenTargets,
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
    let paths: string[];
    try {
      paths = await takePendingOpenTargets();
    } catch (error) {
      if (isCurrentLifecycle(currentLifecycleId)) {
        reportError('Failed to read pending document launch targets:', error);
      }
      return;
    }
    for (const path of paths) {
      if (!isCurrentLifecycle(currentLifecycleId)) return;
      try {
        await pushFilePath(path);
      } catch (error) {
        reportError('Failed to route document launch target:', error);
      }
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
