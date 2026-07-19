import { describe, expect, it, vi } from 'vitest';

import { createApplicationExitCoordinator } from '@/application/applicationExitCoordinator';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}

describe('application exit coordination', () => {
  it('does not execute an exit intent when a guard rejects the request', async () => {
    const execute = vi.fn().mockResolvedValue(undefined);
    const coordinator = createApplicationExitCoordinator({ execute });
    coordinator.registerGuard(vi.fn().mockResolvedValue(false));

    await expect(coordinator.requestExit('close')).resolves.toEqual({ status: 'cancelled' });
    expect(execute).not.toHaveBeenCalled();
  });

  it('upgrades a concurrent close request to the higher-priority relaunch intent', async () => {
    const releaseGuard = deferred<boolean>();
    const execute = vi.fn().mockResolvedValue(undefined);
    const coordinator = createApplicationExitCoordinator({ execute });
    coordinator.registerGuard(() => releaseGuard.promise);

    const close = coordinator.requestExit('close');
    const relaunch = coordinator.requestExit('relaunch');
    releaseGuard.resolve(true);

    await expect(close).resolves.toEqual({ status: 'executed', intent: 'relaunch' });
    await expect(relaunch).resolves.toEqual({ status: 'executed', intent: 'relaunch' });
    expect(execute).toHaveBeenCalledOnce();
    expect(execute).toHaveBeenCalledWith('relaunch');
  });

  it('owns guards and active requests independently per coordinator instance', async () => {
    const firstExecute = vi.fn().mockResolvedValue(undefined);
    const secondExecute = vi.fn().mockResolvedValue(undefined);
    const first = createApplicationExitCoordinator({ execute: firstExecute });
    const second = createApplicationExitCoordinator({ execute: secondExecute });
    first.registerGuard(vi.fn().mockResolvedValue(false));

    await expect(first.requestExit('close')).resolves.toEqual({ status: 'cancelled' });
    await expect(second.requestExit('close')).resolves.toEqual({ status: 'executed', intent: 'close' });
    expect(firstExecute).not.toHaveBeenCalled();
    expect(secondExecute).toHaveBeenCalledOnce();
  });
});
