import { describe, expect, it, vi } from 'vitest';

import { createDocumentPreparationCoordinator } from '@/application/documentPreparationCoordinator';
import type { OperationCancellationSignal } from '@/application/operationCancellation';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}

function controlledCancellation() {
  let cancelled = false;
  const handlers = new Set<() => void>();
  return {
    signal: {
      isCancelled: () => cancelled,
      onCancel(handler: () => void) {
        handlers.add(handler);
        return () => handlers.delete(handler);
      },
    } satisfies OperationCancellationSignal,
    cancel() {
      cancelled = true;
      for (const handler of handlers) handler();
      handlers.clear();
    },
  };
}

async function flushPromises() {
  for (let index = 0; index < 8; index += 1) await Promise.resolve();
}

describe('documentPreparationCoordinator', () => {
  it('keeps required preparation behind a cancelled parse until cleanup drains', async () => {
    const coordinator = createDocumentPreparationCoordinator();
    const firstResult = deferred<{ token: string }>();
    const cancellation = controlledCancellation();
    const discard = vi.fn().mockResolvedValue(undefined);
    const first = coordinator.runCancellable(
      () => firstResult.promise,
      cancellation.signal,
      discard,
    );
    await flushPromises();

    cancellation.cancel();
    await expect(first).resolves.toBeUndefined();
    const requiredPrepare = vi.fn().mockResolvedValue({ token: 'required' });
    const required = coordinator.run(requiredPrepare);
    await flushPromises();
    expect(requiredPrepare).not.toHaveBeenCalled();

    firstResult.resolve({ token: 'cancelled' });

    await expect(required).resolves.toEqual({ token: 'required' });
    expect(discard).toHaveBeenCalledWith({ token: 'cancelled' });
    expect(requiredPrepare).toHaveBeenCalledOnce();
  });

  it('continues the preparation queue after a required task fails', async () => {
    const coordinator = createDocumentPreparationCoordinator();

    await expect(coordinator.run(() => Promise.reject(new Error('parse failed'))))
      .rejects.toThrow('parse failed');
    await expect(coordinator.run(() => Promise.resolve('next'))).resolves.toBe('next');
  });
});
