import type { DocumentSessionLifecycle } from '@/types';

export type DocumentSessionRuntimeState = {
  readonly lifecycle: DocumentSessionLifecycle;
  readonly editorCommandDepth: number;
};

export type DocumentMutationLease = {
  isCurrent(): boolean;
};

export function createDocumentSessionRuntime(
  state: DocumentSessionRuntimeState,
  beginEditorCommand: () => boolean,
  endEditorCommand: () => void,
) {
  let tail: Promise<void> | null = null;
  let generation = 0;
  const interactionIdleWaiters: Array<() => void> = [];

  function beginEditorCommandLease(): (() => void) | null {
    if (!beginEditorCommand()) return null;
    let released = false;
    return () => {
      if (released) return;
      released = true;
      endEditorCommand();
      resolveIdleWaitersIfInteractionIdle();
    };
  }

  function waitForInteractionIdle(): Promise<void> {
    if (isInteractionIdle()) return Promise.resolve();
    return new Promise((resolve) => interactionIdleWaiters.push(resolve));
  }

  function enqueueMutation<T>(
    task: (lease: DocumentMutationLease) => Promise<T>,
  ): Promise<T | undefined> {
    const taskGeneration = generation;
    const previous = tail ?? Promise.resolve();
    const run = previous.then(
      () => runForGeneration(taskGeneration, task),
      () => runForGeneration(taskGeneration, task),
    );
    const cleanup = run.then(() => undefined, () => undefined);
    tail = cleanup;
    void cleanup.finally(() => {
      if (tail === cleanup) tail = null;
    });
    return run;
  }

  function waitForMutations(): Promise<void> {
    return tail ?? Promise.resolve();
  }

  function reset() {
    generation += 1;
    resolveIdleWaitersIfInteractionIdle();
  }

  function notifyInteractionChanged() {
    resolveIdleWaitersIfInteractionIdle();
  }

  async function runForGeneration<T>(
    taskGeneration: number,
    task: (lease: DocumentMutationLease) => Promise<T>,
  ): Promise<T | undefined> {
    const lease = { isCurrent: () => generation === taskGeneration };
    if (!lease.isCurrent()) return undefined;
    try {
      const result = await task(lease);
      return lease.isCurrent() ? result : undefined;
    } catch (error) {
      if (lease.isCurrent()) throw error;
      return undefined;
    }
  }

  function resolveIdleWaitersIfInteractionIdle() {
    if (!isInteractionIdle()) return;
    for (const resolve of interactionIdleWaiters.splice(0)) resolve();
  }

  function isInteractionIdle() {
    return state.lifecycle === 'idle' && state.editorCommandDepth === 0;
  }

  return {
    beginEditorCommandLease,
    waitForInteractionIdle,
    enqueueMutation,
    waitForMutations,
    reset,
    notifyInteractionChanged,
  };
}
