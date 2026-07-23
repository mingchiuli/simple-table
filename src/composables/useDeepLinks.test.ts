import { describe, expect, it, vi } from 'vitest';

import { createDeepLinkLifecycle } from '@/composables/useDeepLinks';

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

async function flushPromises() {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

async function waitForCondition(condition: () => boolean) {
  for (let index = 0; index < 32; index += 1) {
    if (condition()) return;
    await Promise.resolve();
  }
  throw new Error('Timed out waiting for condition');
}

function testDependencies(overrides: Partial<Dependencies> = {}) {
  const handlers: ListenHandler[] = [];
  const unlisten = vi.fn();
  const takePendingOpenTargets = vi.fn().mockResolvedValue([]);
  const pushFilePath = vi.fn().mockResolvedValue(undefined);
  const reportError = vi.fn();
  const dependencies: Dependencies = {
    listen: vi.fn((_event, handler) => {
      handlers.push(handler);
      return Promise.resolve(unlisten);
    }),
    takePendingOpenTargets,
    pushFilePath,
    reportError,
    ...overrides,
  };
  return {
    dependencies,
    getHandler: () => handlers.at(-1) ?? null,
    unlisten,
    takePendingOpenTargets,
    pushFilePath,
    reportError,
  };
}

describe('createDeepLinkLifecycle', () => {
  it('drains backend-normalized startup and live launch targets', async () => {
    const takePendingOpenTargets = vi
      .fn()
      .mockResolvedValueOnce(['/Users/me/start.xlsx'])
      .mockResolvedValueOnce(['C:/Users/me/live.xlsx', '//server/share/opened.xlsx']);
    const setup = testDependencies({ takePendingOpenTargets });
    const lifecycle = createDeepLinkLifecycle(setup.dependencies);

    lifecycle.start();
    await flushPromises();
    setup.getHandler()?.({ payload: null });
    await flushPromises();

    expect(setup.pushFilePath.mock.calls).toEqual([
      ['/Users/me/start.xlsx'],
      ['C:/Users/me/live.xlsx'],
      ['//server/share/opened.xlsx'],
    ]);
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
    expect(setup.takePendingOpenTargets).not.toHaveBeenCalled();
  });

  it('ignores a pending drain after the lifecycle stops', async () => {
    const pendingTargets = deferred<string[]>();
    const takePendingOpenTargets = vi.fn(() => pendingTargets.promise);
    const setup = testDependencies({
      takePendingOpenTargets,
    });
    const lifecycle = createDeepLinkLifecycle(setup.dependencies);

    lifecycle.start();
    await waitForCondition(() => takePendingOpenTargets.mock.calls.length === 1);
    lifecycle.stop();
    pendingTargets.resolve(['/stale.xlsx']);
    await flushPromises();

    expect(setup.pushFilePath).not.toHaveBeenCalled();
  });

  it('serializes drain requests so launch order remains stable', async () => {
    const first = deferred<string[]>();
    const takePendingOpenTargets = vi
      .fn()
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce(['/second.xlsx']);
    const setup = testDependencies({ takePendingOpenTargets });
    const lifecycle = createDeepLinkLifecycle(setup.dependencies);

    lifecycle.start();
    await flushPromises();
    setup.getHandler()?.({ payload: null });
    first.resolve(['/first.xlsx']);
    await flushPromises();

    expect(setup.pushFilePath.mock.calls).toEqual([
      ['/first.xlsx'],
      ['/second.xlsx'],
    ]);
  });

  it('reports drain, routing, and cleanup failures without breaking ownership', async () => {
    const brokenUnlisten = vi.fn(() => {
      throw new Error('broken cleanup');
    });
    const takePendingOpenTargets = vi
      .fn()
      .mockResolvedValueOnce(['/broken.xlsx', '/healthy.xlsx'])
      .mockRejectedValueOnce(new Error('drain failed'));
    const pushFilePath = vi
      .fn()
      .mockRejectedValueOnce(new Error('route failed'))
      .mockResolvedValueOnce(undefined);
    const setup = testDependencies({
      listen: vi.fn((_event, handler) => {
        queueMicrotask(() => handler({ payload: null }));
        return Promise.resolve(brokenUnlisten);
      }),
      takePendingOpenTargets,
      pushFilePath,
    });
    const lifecycle = createDeepLinkLifecycle(setup.dependencies);

    lifecycle.start();
    await waitForCondition(() => setup.reportError.mock.calls.some(
      ([message]) => message === 'Failed to read pending document launch targets:',
    ));
    lifecycle.stop();

    expect(pushFilePath).toHaveBeenCalledTimes(2);
    expect(setup.reportError).toHaveBeenCalledWith(
      'Failed to route document launch target:',
      expect.any(Error),
    );
    expect(setup.reportError).toHaveBeenCalledWith(
      'Failed to read pending document launch targets:',
      expect.any(Error),
    );
    expect(setup.reportError).toHaveBeenCalledWith(
      'Failed to clean up document launch listener:',
      expect.any(Error),
    );
  });
});
