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
});
