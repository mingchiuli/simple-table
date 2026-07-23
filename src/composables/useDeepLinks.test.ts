import { describe, expect, it, vi } from 'vitest';

import { createDeepLinkLifecycle } from '@/composables/useDeepLinks';
import type { OpenTargetClaim } from '@/types/fileRuntime';

type Dependencies = Parameters<typeof createDeepLinkLifecycle>[0];
type Unlisten = () => void;
type ListenHandler = (event: { payload: unknown }) => void;

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}

function claim(claimId: string, path: string): OpenTargetClaim {
  return { claimId, path };
}

async function flushPromises() {
  for (let index = 0; index < 12; index += 1) await Promise.resolve();
}

async function waitForCondition(condition: () => boolean) {
  for (let index = 0; index < 48; index += 1) {
    if (condition()) return;
    await Promise.resolve();
  }
  throw new Error('Timed out waiting for condition');
}

function testDependencies(overrides: Partial<Dependencies> = {}) {
  const handlers: ListenHandler[] = [];
  const unlisten = vi.fn();
  const claimPendingOpenTarget = vi.fn().mockResolvedValue(null);
  const releaseOpenTarget = vi.fn().mockResolvedValue(undefined);
  const pushFilePath = vi.fn().mockResolvedValue(undefined);
  const reportError = vi.fn();
  const dependencies: Dependencies = {
    listen: vi.fn((_event, handler) => {
      handlers.push(handler);
      return Promise.resolve(unlisten);
    }),
    claimPendingOpenTarget,
    releaseOpenTarget,
    pushFilePath,
    reportError,
    ...overrides,
  };
  return {
    dependencies,
    getHandler: () => handlers.at(-1) ?? null,
    unlisten,
    claimPendingOpenTarget,
    releaseOpenTarget,
    pushFilePath,
    reportError,
  };
}

describe('createDeepLinkLifecycle', () => {
  it('claims one backend-normalized target for each startup or live wake', async () => {
    const claimPendingOpenTarget = vi
      .fn()
      .mockResolvedValueOnce(claim('startup', '/Users/me/start.xlsx'))
      .mockResolvedValueOnce(claim('live-1', 'C:/Users/me/live.xlsx'))
      .mockResolvedValueOnce(claim('live-2', '//server/share/opened.xlsx'));
    const setup = testDependencies({ claimPendingOpenTarget });
    const lifecycle = createDeepLinkLifecycle(setup.dependencies);

    lifecycle.start();
    await waitForCondition(() => setup.pushFilePath.mock.calls.length === 1);
    setup.getHandler()?.({ payload: null });
    await waitForCondition(() => setup.pushFilePath.mock.calls.length === 2);
    setup.getHandler()?.({ payload: null });
    await waitForCondition(() => setup.pushFilePath.mock.calls.length === 3);

    expect(setup.pushFilePath.mock.calls).toEqual([
      ['/Users/me/start.xlsx', 'startup'],
      ['C:/Users/me/live.xlsx', 'live-1'],
      ['//server/share/opened.xlsx', 'live-2'],
    ]);
    expect(setup.releaseOpenTarget).not.toHaveBeenCalled();
  });

  it('cleans up a listener that resolves after the lifecycle stops', async () => {
    const pendingListen = deferred<Unlisten>();
    const registeredUnlisten = vi.fn();
    const setup = testDependencies({ listen: vi.fn(() => pendingListen.promise) });
    const lifecycle = createDeepLinkLifecycle(setup.dependencies);

    lifecycle.start();
    lifecycle.stop();
    pendingListen.resolve(registeredUnlisten);
    await flushPromises();

    expect(registeredUnlisten).toHaveBeenCalledOnce();
    expect(setup.claimPendingOpenTarget).not.toHaveBeenCalled();
  });

  it('releases a claim that arrives after the lifecycle stops', async () => {
    const pendingClaim = deferred<OpenTargetClaim | null>();
    const claimPendingOpenTarget = vi.fn(() => pendingClaim.promise);
    const setup = testDependencies({ claimPendingOpenTarget });
    const lifecycle = createDeepLinkLifecycle(setup.dependencies);

    lifecycle.start();
    await waitForCondition(() => claimPendingOpenTarget.mock.calls.length === 1);
    lifecycle.stop();
    pendingClaim.resolve(claim('stale', '/stale.xlsx'));
    await flushPromises();

    expect(setup.pushFilePath).not.toHaveBeenCalled();
    expect(setup.releaseOpenTarget).toHaveBeenCalledWith('stale');
  });

  it('serializes claim requests so launch order remains stable', async () => {
    const first = deferred<OpenTargetClaim | null>();
    const claimPendingOpenTarget = vi
      .fn()
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce(claim('second', '/second.xlsx'))
      .mockResolvedValue(null);
    const setup = testDependencies({ claimPendingOpenTarget });
    const lifecycle = createDeepLinkLifecycle(setup.dependencies);

    lifecycle.start();
    await waitForCondition(() => claimPendingOpenTarget.mock.calls.length === 1);
    setup.getHandler()?.({ payload: null });
    first.resolve(claim('first', '/first.xlsx'));
    await waitForCondition(() => setup.pushFilePath.mock.calls.length === 2);

    expect(setup.pushFilePath.mock.calls).toEqual([
      ['/first.xlsx', 'first'],
      ['/second.xlsx', 'second'],
    ]);
  });

  it('releases a claim when routing fails', async () => {
    const failure = new Error('route failed');
    const setup = testDependencies({
      claimPendingOpenTarget: vi.fn().mockResolvedValueOnce(claim('broken', '/broken.xlsx')),
      pushFilePath: vi.fn().mockRejectedValue(failure),
    });
    const lifecycle = createDeepLinkLifecycle(setup.dependencies);

    lifecycle.start();
    await waitForCondition(() => setup.releaseOpenTarget.mock.calls.length === 1);

    expect(setup.releaseOpenTarget).toHaveBeenCalledWith('broken');
    expect(setup.reportError).toHaveBeenCalledWith(
      'Failed to route document launch target:',
      failure,
    );
  });

  it('reports release failures after a route handoff failure', async () => {
    const routeFailure = new Error('route failed');
    const releaseFailure = new Error('release failed');
    const setup = testDependencies({
      claimPendingOpenTarget: vi.fn().mockResolvedValueOnce(claim('unsettled', '/book.xlsx')),
      pushFilePath: vi.fn().mockRejectedValue(routeFailure),
      releaseOpenTarget: vi.fn().mockRejectedValue(releaseFailure),
    });
    const lifecycle = createDeepLinkLifecycle(setup.dependencies);

    lifecycle.start();
    await waitForCondition(() => setup.reportError.mock.calls.length === 2);

    expect(setup.reportError).toHaveBeenCalledWith(
      'Failed to route document launch target:',
      routeFailure,
    );
    expect(setup.reportError).toHaveBeenCalledWith(
      'Failed to release document launch target:',
      releaseFailure,
    );
  });
});
