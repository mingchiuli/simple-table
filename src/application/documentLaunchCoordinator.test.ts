import { describe, expect, it, vi } from 'vitest';

import { createDocumentLaunchCoordinator } from '@/application/documentLaunchCoordinator';
import type { OpenTargetClaim } from '@/types/fileRuntime';

type Dependencies = Parameters<typeof createDocumentLaunchCoordinator>[0];
type Unlisten = () => void;
type ListenHandler = () => void;
type TestOverrides = Partial<{
  onLaunchTargetAvailable: Dependencies['launchTargets']['onLaunchTargetAvailable'];
  claimPendingOpenTarget: Dependencies['launchTargets']['claimPendingOpenTarget'];
  acknowledgeOpenTarget: Dependencies['launchTargets']['acknowledgeOpenTarget'];
  releaseOpenTarget: Dependencies['launchTargets']['releaseOpenTarget'];
  openTarget: Dependencies['openTarget'];
  reportError: Dependencies['reportError'];
}>;

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
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

function testDependencies(overrides: TestOverrides = {}) {
  const handlers: ListenHandler[] = [];
  const unlisten = vi.fn();
  const claimPendingOpenTarget = vi.fn().mockResolvedValue(null);
  const acknowledgeOpenTarget = vi.fn().mockResolvedValue(undefined);
  const releaseOpenTarget = vi.fn().mockResolvedValue(undefined);
  const pushFilePath = vi.fn().mockResolvedValue(undefined);
  const reportError = vi.fn();
  const dependencies: Dependencies = {
    launchTargets: {
      onLaunchTargetAvailable: overrides.onLaunchTargetAvailable ?? vi.fn((handler) => {
        handlers.push(handler);
        return Promise.resolve(unlisten);
      }),
      claimPendingOpenTarget: overrides.claimPendingOpenTarget ?? claimPendingOpenTarget,
      acknowledgeOpenTarget: overrides.acknowledgeOpenTarget ?? acknowledgeOpenTarget,
      releaseOpenTarget: overrides.releaseOpenTarget ?? releaseOpenTarget,
    },
    openTarget: overrides.openTarget ?? pushFilePath,
    reportError: overrides.reportError ?? reportError,
  };
  return {
    dependencies,
    getHandler: () => handlers.at(-1) ?? null,
    unlisten,
    claimPendingOpenTarget,
    acknowledgeOpenTarget,
    releaseOpenTarget,
    pushFilePath,
    reportError,
  };
}

describe('document launch coordinator', () => {
  it('claims one backend-normalized target for each startup or live wake', async () => {
    const claimPendingOpenTarget = vi
      .fn()
      .mockResolvedValueOnce(claim('startup', '/Users/me/start.xlsx'))
      .mockResolvedValueOnce(claim('live-1', 'C:/Users/me/live.xlsx'))
      .mockResolvedValueOnce(claim('live-2', '//server/share/opened.xlsx'));
    const setup = testDependencies({ claimPendingOpenTarget });
    const lifecycle = createDocumentLaunchCoordinator(setup.dependencies);

    lifecycle.start();
    await waitForCondition(() => setup.pushFilePath.mock.calls.length === 1);
    setup.getHandler()?.();
    await waitForCondition(() => setup.pushFilePath.mock.calls.length === 2);
    setup.getHandler()?.();
    await waitForCondition(() => setup.pushFilePath.mock.calls.length === 3);

    expect(setup.pushFilePath.mock.calls).toEqual([
      ['/Users/me/start.xlsx', 'startup'],
      ['C:/Users/me/live.xlsx', 'live-1'],
      ['//server/share/opened.xlsx', 'live-2'],
    ]);
    expect(setup.acknowledgeOpenTarget).not.toHaveBeenCalled();
    expect(setup.releaseOpenTarget).not.toHaveBeenCalled();
  });

  it('cleans up a listener that resolves after the lifecycle stops', async () => {
    const pendingListen = deferred<Unlisten>();
    const registeredUnlisten = vi.fn();
    const setup = testDependencies({
      onLaunchTargetAvailable: vi.fn(() => pendingListen.promise),
    });
    const lifecycle = createDocumentLaunchCoordinator(setup.dependencies);

    lifecycle.start();
    const disposal = lifecycle.dispose();
    pendingListen.resolve(registeredUnlisten);
    await disposal;

    expect(registeredUnlisten).toHaveBeenCalledOnce();
    expect(setup.claimPendingOpenTarget).not.toHaveBeenCalled();
  });

  it('releases a claim that arrives after the lifecycle stops', async () => {
    const pendingClaim = deferred<OpenTargetClaim | null>();
    const claimPendingOpenTarget = vi.fn(() => pendingClaim.promise);
    const setup = testDependencies({ claimPendingOpenTarget });
    const lifecycle = createDocumentLaunchCoordinator(setup.dependencies);

    lifecycle.start();
    await waitForCondition(() => claimPendingOpenTarget.mock.calls.length === 1);
    const disposal = lifecycle.dispose();
    pendingClaim.resolve(claim('stale', '/stale.xlsx'));
    await disposal;

    expect(setup.pushFilePath).not.toHaveBeenCalled();
    expect(setup.acknowledgeOpenTarget).not.toHaveBeenCalled();
    expect(setup.releaseOpenTarget).toHaveBeenCalledWith('stale');
  });

  it('waits for an active route handoff and releases its claim when the handoff fails', async () => {
    const handoff = deferred<void>();
    const routeFailure = new Error('route stopped');
    const openTarget = vi.fn(() => handoff.promise);
    const setup = testDependencies({
      claimPendingOpenTarget: vi.fn().mockResolvedValueOnce(claim('active', '/active.xlsx')),
      openTarget,
    });
    const lifecycle = createDocumentLaunchCoordinator(setup.dependencies);

    lifecycle.start();
    await waitForCondition(() => openTarget.mock.calls.length === 1);
    let disposed = false;
    const disposal = lifecycle.dispose().then(() => { disposed = true; });
    await flushPromises();

    expect(disposed).toBe(false);
    expect(setup.acknowledgeOpenTarget).not.toHaveBeenCalled();
    expect(setup.releaseOpenTarget).not.toHaveBeenCalled();

    handoff.reject(routeFailure);
    await disposal;

    expect(setup.releaseOpenTarget).toHaveBeenCalledWith('active');
    expect(setup.reportError).toHaveBeenCalledWith(
      'Failed to route document launch target:',
      routeFailure,
    );
  });

  it('serializes claim requests so launch order remains stable', async () => {
    const first = deferred<OpenTargetClaim | null>();
    const claimPendingOpenTarget = vi
      .fn()
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce(claim('second', '/second.xlsx'))
      .mockResolvedValue(null);
    const setup = testDependencies({ claimPendingOpenTarget });
    const lifecycle = createDocumentLaunchCoordinator(setup.dependencies);

    lifecycle.start();
    await waitForCondition(() => claimPendingOpenTarget.mock.calls.length === 1);
    setup.getHandler()?.();
    first.resolve(claim('first', '/first.xlsx'));
    await waitForCondition(() => setup.pushFilePath.mock.calls.length === 2);

    expect(setup.pushFilePath.mock.calls).toEqual([
      ['/first.xlsx', 'first'],
      ['/second.xlsx', 'second'],
    ]);
  });

  it('coalesces a burst of wake events while a claim is in flight', async () => {
    const first = deferred<OpenTargetClaim | null>();
    const claimPendingOpenTarget = vi
      .fn()
      .mockReturnValueOnce(first.promise)
      .mockResolvedValue(null);
    const setup = testDependencies({ claimPendingOpenTarget });
    const lifecycle = createDocumentLaunchCoordinator(setup.dependencies);

    lifecycle.start();
    await waitForCondition(() => claimPendingOpenTarget.mock.calls.length === 1);
    for (let index = 0; index < 10_000; index += 1) setup.getHandler()?.();
    await flushPromises();
    expect(claimPendingOpenTarget).toHaveBeenCalledOnce();

    first.resolve(claim('first', '/first.xlsx'));
    await waitForCondition(() => claimPendingOpenTarget.mock.calls.length === 2);
    await flushPromises();

    expect(claimPendingOpenTarget).toHaveBeenCalledTimes(2);
    expect(setup.pushFilePath).toHaveBeenCalledOnce();
  });

  it('acknowledges a claim when routing fails while the lifecycle is active', async () => {
    const failure = new Error('route failed');
    const setup = testDependencies({
      claimPendingOpenTarget: vi.fn().mockResolvedValueOnce(claim('broken', '/broken.xlsx')),
      openTarget: vi.fn().mockRejectedValue(failure),
    });
    const lifecycle = createDocumentLaunchCoordinator(setup.dependencies);

    lifecycle.start();
    await waitForCondition(() => setup.acknowledgeOpenTarget.mock.calls.length === 1);

    expect(setup.acknowledgeOpenTarget).toHaveBeenCalledWith('broken');
    expect(setup.releaseOpenTarget).not.toHaveBeenCalled();
    expect(setup.reportError).toHaveBeenCalledWith(
      'Failed to route document launch target:',
      failure,
    );
  });

  it('reports acknowledgement failures after a route handoff failure', async () => {
    const routeFailure = new Error('route failed');
    const acknowledgementFailure = new Error('acknowledgement failed');
    const setup = testDependencies({
      claimPendingOpenTarget: vi.fn().mockResolvedValueOnce(claim('unsettled', '/book.xlsx')),
      openTarget: vi.fn().mockRejectedValue(routeFailure),
      acknowledgeOpenTarget: vi.fn().mockRejectedValue(acknowledgementFailure),
    });
    const lifecycle = createDocumentLaunchCoordinator(setup.dependencies);

    lifecycle.start();
    await waitForCondition(() => setup.reportError.mock.calls.length === 2);

    expect(setup.reportError).toHaveBeenCalledWith(
      'Failed to route document launch target:',
      routeFailure,
    );
    expect(setup.reportError).toHaveBeenCalledWith(
      'Failed to acknowledge document launch target:',
      acknowledgementFailure,
    );
    await expect(lifecycle.dispose()).rejects.toMatchObject({
      name: 'AggregateError',
      message: 'Failed to completely dispose document launch coordination',
      errors: [acknowledgementFailure],
    });
  });
});
