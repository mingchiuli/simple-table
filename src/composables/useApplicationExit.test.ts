import { describe, expect, it, vi } from 'vitest';

import { createApplicationExitCoordinator } from '@/application/applicationExitCoordinator';
import { createWindowCloseGuardLifecycle } from '@/composables/useApplicationExit';

function preparation() {
  return {
    commit: vi.fn(),
    rollback: vi.fn(),
  };
}

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
    coordinator.registerGuard(vi.fn().mockResolvedValue(null));

    await expect(coordinator.requestExit('close')).resolves.toEqual({ status: 'cancelled' });
    expect(execute).not.toHaveBeenCalled();
  });

  it('upgrades a concurrent close request to the higher-priority relaunch intent', async () => {
    const releaseGuard = deferred<boolean>();
    const execute = vi.fn().mockResolvedValue(undefined);
    const coordinator = createApplicationExitCoordinator({ execute });
    const guardPreparation = preparation();
    coordinator.registerGuard(async () => (await releaseGuard.promise) ? guardPreparation : null);

    const close = coordinator.requestExit('close');
    const relaunch = coordinator.requestExit('relaunch');
    releaseGuard.resolve(true);

    await expect(close).resolves.toEqual({ status: 'executed', intent: 'relaunch' });
    await expect(relaunch).resolves.toEqual({ status: 'executed', intent: 'relaunch' });
    expect(execute).toHaveBeenCalledOnce();
    expect(execute).toHaveBeenCalledWith('relaunch');
    expect(guardPreparation.commit).toHaveBeenCalledOnce();
    expect(guardPreparation.rollback).not.toHaveBeenCalled();
  });

  it('owns guards and active requests independently per coordinator instance', async () => {
    const firstExecute = vi.fn().mockResolvedValue(undefined);
    const secondExecute = vi.fn().mockResolvedValue(undefined);
    const first = createApplicationExitCoordinator({ execute: firstExecute });
    const second = createApplicationExitCoordinator({ execute: secondExecute });
    first.registerGuard(vi.fn().mockResolvedValue(null));

    await expect(first.requestExit('close')).resolves.toEqual({ status: 'cancelled' });
    await expect(second.requestExit('close')).resolves.toEqual({ status: 'executed', intent: 'close' });
    expect(firstExecute).not.toHaveBeenCalled();
    expect(secondExecute).toHaveBeenCalledOnce();
  });

  it('keeps exit preparations active until execution succeeds', async () => {
    const execution = deferred<void>();
    const guardPreparation = preparation();
    const coordinator = createApplicationExitCoordinator({
      execute: vi.fn(() => execution.promise),
    });
    coordinator.registerGuard(vi.fn().mockResolvedValue(guardPreparation));

    const exit = coordinator.requestExit('close');
    await Promise.resolve();

    expect(guardPreparation.commit).not.toHaveBeenCalled();
    expect(guardPreparation.rollback).not.toHaveBeenCalled();

    execution.resolve();
    await expect(exit).resolves.toEqual({ status: 'executed', intent: 'close' });
    expect(guardPreparation.commit).toHaveBeenCalledOnce();
    expect(guardPreparation.rollback).not.toHaveBeenCalled();
  });

  it('rolls back prepared guards in reverse order when execution fails', async () => {
    const order: string[] = [];
    const coordinator = createApplicationExitCoordinator({
      execute: vi.fn().mockRejectedValue(new Error('destroy failed')),
    });
    coordinator.registerGuard(vi.fn().mockResolvedValue({
      commit: () => order.push('first-commit'),
      rollback: () => order.push('first-rollback'),
    }));
    coordinator.registerGuard(vi.fn().mockResolvedValue({
      commit: () => order.push('second-commit'),
      rollback: () => order.push('second-rollback'),
    }));

    await expect(coordinator.requestExit('close')).rejects.toThrow('destroy failed');

    expect(order).toEqual(['first-rollback', 'second-rollback']);
  });

  it('cancels a guard in progress and waits for it during disposal', async () => {
    const guardResult = deferred<boolean>();
    const guardPreparation = preparation();
    const execute = vi.fn().mockResolvedValue(undefined);
    const coordinator = createApplicationExitCoordinator({ execute });
    coordinator.registerGuard(async () => (
      await guardResult.promise ? guardPreparation : null
    ));

    const exit = coordinator.requestExit('close');
    const disposal = coordinator.dispose();
    let disposed = false;
    void disposal.then(() => { disposed = true; });
    await Promise.resolve();
    expect(disposed).toBe(false);

    guardResult.resolve(true);
    await expect(exit).resolves.toEqual({ status: 'cancelled' });
    await disposal;

    expect(guardPreparation.rollback).toHaveBeenCalledOnce();
    expect(execute).not.toHaveBeenCalled();
    await expect(coordinator.requestExit('relaunch')).resolves.toEqual({ status: 'cancelled' });
  });
});

describe('window close guard lifecycle', () => {
  it('forwards close requests through the coordinator and unregisters on disposal', async () => {
    const closeHandlers: Array<() => void | Promise<void>> = [];
    const unregister = vi.fn();
    const requestExit = vi.fn().mockResolvedValue({ status: 'executed', intent: 'close' });
    const lifecycle = createWindowCloseGuardLifecycle(
      {
        subscribeCloseRequested: vi.fn(async (handler) => {
          closeHandlers.push(handler);
          return unregister;
        }),
      },
      { requestExit },
    );

    await lifecycle.start();
    expect(closeHandlers).toHaveLength(1);
    await closeHandlers[0]?.();

    expect(requestExit).toHaveBeenCalledOnce();
    expect(requestExit).toHaveBeenCalledWith('close');
    lifecycle.dispose();
    expect(unregister).toHaveBeenCalledOnce();
  });

  it('unregisters a delayed subscription that resolves after disposal', async () => {
    const subscription = deferred<() => void>();
    const unregister = vi.fn();
    const lifecycle = createWindowCloseGuardLifecycle(
      { subscribeCloseRequested: vi.fn(() => subscription.promise) },
      { requestExit: vi.fn() },
    );

    const start = lifecycle.start();
    lifecycle.dispose();
    subscription.resolve(unregister);
    await start;

    expect(unregister).toHaveBeenCalledOnce();
  });

  it('reports subscription and exit failures through the presentation boundary', async () => {
    const reportError = vi.fn();
    const failedRegistration = createWindowCloseGuardLifecycle(
      { subscribeCloseRequested: vi.fn().mockRejectedValue(new Error('listen failed')) },
      { requestExit: vi.fn() },
      reportError,
    );
    await failedRegistration.start();

    const closeHandlers: Array<() => void | Promise<void>> = [];
    const failedExit = createWindowCloseGuardLifecycle(
      {
        subscribeCloseRequested: vi.fn(async (handler) => {
          closeHandlers.push(handler);
          return () => undefined;
        }),
      },
      { requestExit: vi.fn().mockRejectedValue(new Error('close failed')) },
      reportError,
    );
    await failedExit.start();
    expect(closeHandlers).toHaveLength(1);
    await closeHandlers[0]?.();

    expect(reportError).toHaveBeenCalledTimes(2);
    expect(reportError.mock.calls[0]?.[0]).toContain('register');
    expect(reportError.mock.calls[1]?.[0]).toContain('close');
  });
});
