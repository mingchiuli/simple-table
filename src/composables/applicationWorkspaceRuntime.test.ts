import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { ApplicationExitCoordinator } from '@/application/applicationExitCoordinator';
import {
  createApplicationWorkspaceRuntime,
} from '@/composables/applicationWorkspaceRuntime';
import type { DocumentWorkspaceRuntime } from '@/composables/documentWorkspaceRuntime';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}

describe('applicationWorkspaceRuntime', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it('waits for every child disposal before reporting aggregated failures', async () => {
    const documentDrain = deferred<void>();
    const documentLaunchDrain = deferred<void>();
    const exitFailure = new Error('exit cleanup failed');
    const applicationExit = {
      registerGuard: vi.fn(() => () => undefined),
      requestExit: vi.fn().mockResolvedValue({ status: 'cancelled' }),
      dispose: vi.fn(() => {
        throw exitFailure;
      }),
    } as unknown as ApplicationExitCoordinator;
    const document = {
      dispose: vi.fn(() => documentDrain.promise),
    } as unknown as DocumentWorkspaceRuntime;
    const runtime = createApplicationWorkspaceRuntime({ applicationExit, document });
    const documentLaunch = {
      start: vi.fn(),
      dispose: vi.fn(() => documentLaunchDrain.promise),
    };
    runtime.registerDocumentLaunch(documentLaunch);

    let settled = false;
    const disposal = runtime.dispose();
    void disposal.then(
      () => { settled = true; },
      () => { settled = true; },
    );
    await Promise.resolve();

    expect(applicationExit.dispose).toHaveBeenCalledOnce();
    expect(document.dispose).toHaveBeenCalledOnce();
    expect(documentLaunch.dispose).toHaveBeenCalledOnce();
    expect(settled).toBe(false);

    documentDrain.resolve();
    await Promise.resolve();
    expect(settled).toBe(false);

    documentLaunchDrain.resolve();
    await expect(disposal).rejects.toMatchObject({
      name: 'AggregateError',
      message: 'Failed to completely drain the application workspace',
      errors: [exitFailure],
    });
  });
});
