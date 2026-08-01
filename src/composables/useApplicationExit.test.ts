import { describe, expect, it, vi } from 'vitest';

import {
  createApplicationExitCoordinator,
  createWindowCloseRequestLifecycle,
} from '@/application/applicationExitCoordinator';

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

  it('commits every preparation even when an earlier commit fails', async () => {
    const order: string[] = [];
    const commitFailure = new Error('second commit failed');
    const coordinator = createApplicationExitCoordinator({
      execute: vi.fn().mockResolvedValue(undefined),
    });
    coordinator.registerGuard(vi.fn().mockResolvedValue({
      commit: () => order.push('first-commit'),
      rollback: vi.fn(),
    }));
    coordinator.registerGuard(vi.fn().mockResolvedValue({
      commit: () => {
        order.push('second-commit');
        throw commitFailure;
      },
      rollback: vi.fn(),
    }));
    coordinator.registerGuard(vi.fn().mockResolvedValue({
      commit: () => order.push('third-commit'),
      rollback: vi.fn(),
    }));

    await expect(coordinator.requestExit('close')).rejects.toMatchObject({
      name: 'AggregateError',
      message: 'Application exit executed but one or more preparations failed to commit',
      errors: [commitFailure],
    });
    expect(order).toEqual(['third-commit', 'second-commit', 'first-commit']);
  });

  it('preserves the execution error and rolls back every preparation after one rollback fails', async () => {
    const order: string[] = [];
    const executionFailure = new Error('destroy failed');
    const rollbackFailure = new Error('second rollback failed');
    const coordinator = createApplicationExitCoordinator({
      execute: vi.fn().mockRejectedValue(executionFailure),
    });
    coordinator.registerGuard(vi.fn().mockResolvedValue({
      commit: vi.fn(),
      rollback: () => order.push('first-rollback'),
    }));
    coordinator.registerGuard(vi.fn().mockResolvedValue({
      commit: vi.fn(),
      rollback: () => {
        order.push('second-rollback');
        throw rollbackFailure;
      },
    }));
    coordinator.registerGuard(vi.fn().mockResolvedValue({
      commit: vi.fn(),
      rollback: () => order.push('third-rollback'),
    }));

    await expect(coordinator.requestExit('close')).rejects.toMatchObject({
      name: 'AggregateError',
      message: 'Application exit execution failed and rollback was incomplete',
      errors: [executionFailure, rollbackFailure],
    });
    expect(order).toEqual(['first-rollback', 'second-rollback', 'third-rollback']);
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

  it('times out a stuck guard and rolls back a preparation that arrives late', async () => {
    vi.useFakeTimers();
    try {
      const lateGuard = deferred<ReturnType<typeof preparation>>();
      const latePreparation = preparation();
      const coordinator = createApplicationExitCoordinator(
        { execute: vi.fn() },
        { guardTimeoutMs: 100 },
      );
      coordinator.registerGuard(() => lateGuard.promise);

      const exit = coordinator.requestExit('close');
      const rejectedExit = expect(exit).rejects.toThrow('timed out after 100 ms');
      await vi.advanceTimersByTimeAsync(100);
      await rejectedExit;

      lateGuard.resolve(latePreparation);
      await vi.waitFor(() => {
        expect(latePreparation.rollback).toHaveBeenCalledOnce();
      });
    } finally {
      vi.useRealTimers();
    }
  });
});

describe('window close guard lifecycle', () => {
  it('forwards close requests through the coordinator and unregisters on disposal', async () => {
    const closeHandlers: Array<() => void | Promise<void>> = [];
    const unregister = vi.fn();
    const requestExit = vi.fn().mockResolvedValue({ status: 'executed', intent: 'close' });
    const lifecycle = createWindowCloseRequestLifecycle(
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
    const lifecycle = createWindowCloseRequestLifecycle(
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
    const requestExitAfterFailedRegistration = vi.fn();
    const failedSubscription = vi.fn().mockRejectedValue(new Error('listen failed'));
    let failedRegistration: ReturnType<typeof createWindowCloseRequestLifecycle>;
    failedRegistration = createWindowCloseRequestLifecycle(
      { subscribeCloseRequested: failedSubscription },
      { requestExit: requestExitAfterFailedRegistration },
      (message, error) => {
        reportError(message, error);
        failedRegistration.dispose();
      },
      () => Promise.resolve(),
    );
    await failedRegistration.start();
    expect(failedSubscription).toHaveBeenCalledTimes(3);
    expect(requestExitAfterFailedRegistration).not.toHaveBeenCalled();

    const closeHandlers: Array<() => void | Promise<void>> = [];
    const failedExit = createWindowCloseRequestLifecycle(
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

  it('retries a transient close-listener registration failure', async () => {
    const closeHandlers: Array<() => void | Promise<void>> = [];
    const unregister = vi.fn();
    const subscribeCloseRequested = vi
      .fn()
      .mockRejectedValueOnce(new Error('listen failed once'))
      .mockImplementation(async (handler) => {
        closeHandlers.push(handler);
        return unregister;
      });
    const reportError = vi.fn();
    const lifecycle = createWindowCloseRequestLifecycle(
      { subscribeCloseRequested },
      { requestExit: vi.fn().mockResolvedValue({ status: 'executed', intent: 'close' }) },
      reportError,
      () => Promise.resolve(),
    );

    await lifecycle.start();

    expect(subscribeCloseRequested).toHaveBeenCalledTimes(2);
    expect(closeHandlers).toHaveLength(1);
    expect(reportError).not.toHaveBeenCalled();
    lifecycle.dispose();
    expect(unregister).toHaveBeenCalledOnce();
  });

  it('keeps retrying close-listener registration after the first reported failure batch', async () => {
    const unregister = vi.fn();
    const subscribeCloseRequested = vi
      .fn()
      .mockRejectedValueOnce(new Error('listen failed 1'))
      .mockRejectedValueOnce(new Error('listen failed 2'))
      .mockRejectedValueOnce(new Error('listen failed 3'))
      .mockResolvedValue(unregister);
    const reportError = vi.fn();
    const lifecycle = createWindowCloseRequestLifecycle(
      { subscribeCloseRequested },
      { requestExit: vi.fn() },
      reportError,
      () => Promise.resolve(),
    );

    await lifecycle.start();

    expect(subscribeCloseRequested).toHaveBeenCalledTimes(4);
    expect(reportError).toHaveBeenCalledOnce();
    lifecycle.dispose();
    expect(unregister).toHaveBeenCalledOnce();
  });

  it('abandons a stuck registration attempt and unregisters it if it resolves late', async () => {
    vi.useFakeTimers();
    try {
      const stuckRegistration = deferred<() => void>();
      const lateUnregister = vi.fn();
      const activeUnregister = vi.fn();
      const subscribeCloseRequested = vi
        .fn()
        .mockReturnValueOnce(stuckRegistration.promise)
        .mockResolvedValue(activeUnregister);
      const lifecycle = createWindowCloseRequestLifecycle(
        { subscribeCloseRequested },
        { requestExit: vi.fn() },
        vi.fn(),
        () => Promise.resolve(),
        100,
      );

      const started = lifecycle.start();
      await vi.advanceTimersByTimeAsync(100);
      await started;
      expect(subscribeCloseRequested).toHaveBeenCalledTimes(2);

      stuckRegistration.resolve(lateUnregister);
      await vi.waitFor(() => {
        expect(lateUnregister).toHaveBeenCalledOnce();
      });
      lifecycle.dispose();
      expect(activeUnregister).toHaveBeenCalledOnce();
    } finally {
      vi.useRealTimers();
    }
  });
});
