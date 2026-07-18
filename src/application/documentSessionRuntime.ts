import type { DocumentSessionLifecycle } from '@/types';

export type DocumentSessionRuntimeState = {
  readonly lifecycle: DocumentSessionLifecycle;
  readonly editorCommandDepth: number;
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

  function enqueueMutation<T>(task: () => Promise<T>): Promise<T | undefined> {
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
    tail = null;
    resolveIdleWaitersIfInteractionIdle();
  }

  function notifyInteractionChanged() {
    resolveIdleWaitersIfInteractionIdle();
  }

  function runForGeneration<T>(taskGeneration: number, task: () => Promise<T>) {
    return generation === taskGeneration ? task() : Promise.resolve(undefined);
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
