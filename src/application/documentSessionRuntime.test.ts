import { describe, expect, it, vi } from 'vitest';

import { createDocumentSessionRuntime } from '@/application/documentSessionRuntime';

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

describe('documentSessionRuntime', () => {
  it('keeps an active mutation in the serialization chain across reset', async () => {
    const releaseFirst = deferred<void>();
    const state = { lifecycle: 'idle' as const, editorCommandDepth: 0 };
    const runtime = createDocumentSessionRuntime(state, () => true, () => undefined);
    let active = 0;
    let peak = 0;
    const first = runtime.enqueueMutation(async () => {
      active += 1;
      peak = Math.max(peak, active);
      await releaseFirst.promise;
      active -= 1;
      return 'old';
    });
    await flushPromises();

    runtime.reset();
    const secondTask = vi.fn(async () => {
      active += 1;
      peak = Math.max(peak, active);
      active -= 1;
      return 'new';
    });
    const second = runtime.enqueueMutation(secondTask);
    let drained = false;
    void runtime.waitForMutations().then(() => { drained = true; });
    await flushPromises();

    expect(secondTask).not.toHaveBeenCalled();
    expect(drained).toBe(false);

    releaseFirst.resolve();
    await expect(first).resolves.toBeUndefined();
    await expect(second).resolves.toBe('new');
    await runtime.waitForMutations();

    expect(peak).toBe(1);
    expect(drained).toBe(true);
  });

  it('skips mutations queued in a retired generation', async () => {
    const releaseFirst = deferred<void>();
    const runtime = createDocumentSessionRuntime(
      { lifecycle: 'idle', editorCommandDepth: 0 },
      () => true,
      () => undefined,
    );
    const first = runtime.enqueueMutation(async () => releaseFirst.promise);
    const retiredTask = vi.fn(async () => 'retired');
    const retired = runtime.enqueueMutation(retiredTask);
    await flushPromises();

    runtime.reset();
    const currentTask = vi.fn(async () => 'current');
    const current = runtime.enqueueMutation(currentTask);
    releaseFirst.resolve();

    await Promise.all([first, retired]);
    await expect(current).resolves.toBe('current');
    expect(retiredTask).not.toHaveBeenCalled();
    expect(currentTask).toHaveBeenCalledOnce();
  });
});
