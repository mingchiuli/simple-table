import { createPinia, setActivePinia } from 'pinia';
import { effectScope } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { createRouteLeaveHandler, useRouteFileLoader } from '@/composables/useRouteFileLoader';
import { createApplicationWorkspaceTestContext } from '@/test/documentWorkspaceTestContext';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}

async function flushPromises() {
  for (let index = 0; index < 8; index += 1) await Promise.resolve();
}

describe('createRouteLeaveHandler', () => {
  it('does not dispose route file loading when route leave is rejected', async () => {
    const routeFileLoader = { dispose: vi.fn().mockResolvedValue(undefined) };
    const closeCurrentDocument = vi.fn().mockResolvedValue(false);
    const leave = createRouteLeaveHandler({
      routeFileLoader,
      hasActiveDocument: () => true,
      closeCurrentDocument,
    });

    await expect(leave()).resolves.toBe(false);

    expect(closeCurrentDocument).toHaveBeenCalledTimes(1);
    expect(routeFileLoader.dispose).not.toHaveBeenCalled();
  });

  it('disposes route file loading after route leave is accepted', async () => {
    const routeFileLoader = { dispose: vi.fn().mockResolvedValue(undefined) };
    const closeCurrentDocument = vi.fn().mockResolvedValue(true);
    const leave = createRouteLeaveHandler({
      routeFileLoader,
      hasActiveDocument: () => true,
      closeCurrentDocument,
    });

    await expect(leave()).resolves.toBe(true);

    expect(routeFileLoader.dispose).toHaveBeenCalledTimes(1);
  });

  it('disposes route file loading immediately when no document is active', async () => {
    const routeFileLoader = { dispose: vi.fn().mockResolvedValue(undefined) };
    const closeCurrentDocument = vi.fn();
    const leave = createRouteLeaveHandler({
      routeFileLoader,
      hasActiveDocument: () => false,
      closeCurrentDocument,
    });

    await expect(leave()).resolves.toBe(true);

    expect(closeCurrentDocument).not.toHaveBeenCalled();
    expect(routeFileLoader.dispose).toHaveBeenCalledTimes(1);
  });

  it('continues an accepted route leave after disposal reports a cleanup failure', async () => {
    const cleanupFailure = new Error('route cleanup failed');
    const routeFileLoader = { dispose: vi.fn().mockRejectedValue(cleanupFailure) };
    const report = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const leave = createRouteLeaveHandler({
      routeFileLoader,
      hasActiveDocument: () => false,
      closeCurrentDocument: vi.fn(),
    });

    await expect(leave()).resolves.toBe(true);
    expect(report).toHaveBeenCalledWith('Failed to dispose route document loading:', cleanupFailure);
    report.mockRestore();
  });
});

describe('useRouteFileLoader', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it('keeps route loading owned by the application until its drain settles', async () => {
    const workspace = createApplicationWorkspaceTestContext();
    const load = deferred<boolean>();
    const claimSettlement = deferred<void>();
    const releaseOpenTarget = vi.fn(() => claimSettlement.promise);
    const scope = effectScope();
    const loader = workspace.run(() => scope.run(() => useRouteFileLoader({
      getRouteFilePath: () => '/tmp/owned.xlsx',
      getRouteOpenTargetClaimId: () => 'owned-claim',
      getCurrentFilePath: () => null,
      loadFileFromPath: () => load.promise,
      refreshEditorState: vi.fn().mockResolvedValue(undefined),
      acknowledgeOpenTarget: vi.fn().mockResolvedValue(undefined),
      releaseOpenTarget,
      reportError: vi.fn(),
    })))!;
    loader.enqueue('/tmp/owned.xlsx', 'owned-claim');
    await flushPromises();

    scope.stop();
    let applicationDisposed = false;
    const disposal = workspace.application.dispose().then(() => {
      applicationDisposed = true;
    });
    await flushPromises();
    expect(applicationDisposed).toBe(false);

    load.resolve(false);
    await flushPromises();
    expect(releaseOpenTarget).toHaveBeenCalledWith('owned-claim');
    expect(applicationDisposed).toBe(false);

    claimSettlement.resolve();
    await disposal;
    expect(applicationDisposed).toBe(true);
  });
});
