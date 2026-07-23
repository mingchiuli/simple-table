export type WorkspaceOperationTracker = ReturnType<typeof createWorkspaceOperationTracker>;

export class WorkspaceDisposedError extends Error {
  constructor() {
    super('Document workspace is no longer accepting work');
    this.name = 'WorkspaceDisposedError';
  }
}

export function createWorkspaceOperationTracker() {
  let state: 'active' | 'disposing' | 'disposed' = 'active';
  let activeOperations = 0;
  const idleWaiters: Array<() => void> = [];

  function isAcceptingWork() {
    return state === 'active';
  }

  function run<T>(task: () => Promise<T>, disposedValue: T): Promise<T> {
    const release = begin();
    if (!release) return Promise.resolve(disposedValue);
    return runTracked(task, release);
  }

  function runRequired<T>(task: () => Promise<T>): Promise<T> {
    const release = begin();
    if (!release) return Promise.reject(new WorkspaceDisposedError());
    return runTracked(task, release);
  }

  function stopAcceptingWork() {
    if (state === 'active') state = 'disposing';
  }

  function waitForIdle(): Promise<void> {
    if (activeOperations === 0) return Promise.resolve();
    return new Promise((resolve) => idleWaiters.push(resolve));
  }

  function markDisposed() {
    stopAcceptingWork();
    state = 'disposed';
    resolveIdleWaiters();
  }

  function begin(): (() => void) | null {
    if (!isAcceptingWork()) return null;
    activeOperations += 1;
    let released = false;
    return () => {
      if (released) return;
      released = true;
      activeOperations = Math.max(0, activeOperations - 1);
      resolveIdleWaiters();
    };
  }

  async function runTracked<T>(task: () => Promise<T>, release: () => void): Promise<T> {
    try {
      return await task();
    } finally {
      release();
    }
  }

  function resolveIdleWaiters() {
    if (activeOperations !== 0) return;
    for (const resolve of idleWaiters.splice(0)) resolve();
  }

  return {
    isAcceptingWork,
    run,
    runRequired,
    stopAcceptingWork,
    waitForIdle,
    markDisposed,
  };
}
