import { describe, expect, it, vi } from 'vitest';

import {
  createWorkspaceOperationTracker,
  WorkspaceDisposedError,
} from '@/application/workspaceOperationTracker';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}

describe('workspaceOperationTracker', () => {
  it('stops new work and waits for already admitted operations', async () => {
    const tracker = createWorkspaceOperationTracker();
    const active = deferred<string>();
    const operation = tracker.runRequired(() => active.promise);

    tracker.stopAcceptingWork();
    const rejectedTask = vi.fn().mockResolvedValue('late');
    await expect(tracker.run(rejectedTask, 'disposed')).resolves.toBe('disposed');
    await expect(tracker.runRequired(rejectedTask)).rejects.toBeInstanceOf(WorkspaceDisposedError);
    expect(rejectedTask).not.toHaveBeenCalled();

    let idle = false;
    const wait = tracker.waitForIdle().then(() => { idle = true; });
    await Promise.resolve();
    expect(idle).toBe(false);

    active.resolve('completed');
    await expect(operation).resolves.toBe('completed');
    await wait;
    expect(idle).toBe(true);
  });

  it('guards async and synchronous functions after disposal starts', async () => {
    const tracker = createWorkspaceOperationTracker();
    const asyncTask = vi.fn(async (value: number) => value + 1);
    const syncTask = vi.fn((value: number) => value + 2);
    const guardedAsync = tracker.guard(asyncTask, -1);
    const guardedRequired = tracker.guardRequired(asyncTask);
    const guardedSync = tracker.guardSync(syncTask, -2);

    await expect(guardedAsync(1)).resolves.toBe(2);
    await expect(guardedRequired(2)).resolves.toBe(3);
    expect(guardedSync(3)).toBe(5);

    tracker.stopAcceptingWork();

    await expect(guardedAsync(4)).resolves.toBe(-1);
    await expect(guardedRequired(5)).rejects.toBeInstanceOf(WorkspaceDisposedError);
    expect(guardedSync(6)).toBe(-2);
    expect(asyncTask).toHaveBeenCalledTimes(2);
    expect(syncTask).toHaveBeenCalledOnce();
  });
});
