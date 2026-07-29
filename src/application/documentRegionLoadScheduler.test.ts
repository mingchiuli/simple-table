import { describe, expect, it } from 'vitest';
import {
  createDocumentRegionLoadScheduler,
} from '@/application/documentRegionLoadScheduler';
import { createOperationCancellationSource } from '@/application/operationCancellation';

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}

describe('documentRegionLoadScheduler', () => {
  it('runs at most four region requests concurrently', async () => {
    const scheduler = createDocumentRegionLoadScheduler();
    let active = 0;
    let peak = 0;
    const releases: Array<() => void> = [];
    const loads = Array.from({ length: 6 }, (_, index) =>
      scheduler.scheduleRegionLoad(`block-${index}`, async () => {
        active += 1;
        peak = Math.max(peak, active);
        await new Promise<void>((resolve) => releases.push(resolve));
        active -= 1;
        return true;
      })
    );

    await waitFor(() => releases.length === 4);
    expect(peak).toBe(4);
    releases.splice(0, 4).forEach((release) => release());
    await waitFor(() => releases.length === 2);
    releases.splice(0).forEach((release) => release());

    await expect(Promise.all(loads)).resolves.toEqual([true, true, true, true, true, true]);
    expect(peak).toBe(4);
  });

  it('bounds admitted viewport requests and rejects excess work', async () => {
    const scheduler = createDocumentRegionLoadScheduler();
    const keys = Array.from({ length: 20 }, (_, index) => `block-${index}`);
    const generation = scheduler.beginViewportRegionLoad(keys);
    let started = 0;
    const releases: Array<() => void> = [];
    const loads = keys.map((key) => scheduler.scheduleRegionLoad(key, async () => {
      started += 1;
      await new Promise<void>((resolve) => releases.push(resolve));
      return true;
    }, { priority: 'viewport', viewportGeneration: generation }));

    await waitFor(() => releases.length === 4);
    await expect(Promise.all(loads.slice(16))).resolves.toEqual([false, false, false, false]);
    while (started < 16) {
      releases.splice(0).forEach((release) => release());
      await waitFor(() => releases.length > 0);
    }
    releases.splice(0).forEach((release) => release());

    const results = await Promise.all(loads);
    expect(results.filter(Boolean)).toHaveLength(16);
    expect(started).toBe(16);
  });

  it('drops queued tiles superseded by the latest viewport', async () => {
    const scheduler = createDocumentRegionLoadScheduler();
    const oldKeys = Array.from({ length: 10 }, (_, index) => `old-${index}`);
    const oldGeneration = scheduler.beginViewportRegionLoad(oldKeys);
    let started = 0;
    const releases: Array<() => void> = [];
    const oldLoads = oldKeys.map((key) => scheduler.scheduleRegionLoad(key, async () => {
      started += 1;
      await new Promise<void>((resolve) => releases.push(resolve));
      return true;
    }, { priority: 'viewport', viewportGeneration: oldGeneration }));
    await waitFor(() => releases.length === 4);

    const currentGeneration = scheduler.beginViewportRegionLoad(['current']);
    const current = scheduler.scheduleRegionLoad('current', async () => {
      started += 1;
      return true;
    }, { priority: 'viewport', viewportGeneration: currentGeneration });
    releases.splice(0).forEach((release) => release());

    expect(await current).toBe(true);
    const oldResults = await Promise.all(oldLoads);
    expect(oldResults.filter(Boolean)).toHaveLength(0);
    expect(started).toBe(5);
  });

  it('keeps an active tile current when the next viewport retains it', async () => {
    const scheduler = createDocumentRegionLoadScheduler();
    const release = deferred<void>();
    const firstGeneration = scheduler.beginViewportRegionLoad(['shared']);
    let started = 0;
    const first = scheduler.scheduleRegionLoad('shared', async (isCurrent) => {
      started += 1;
      await release.promise;
      return isCurrent();
    }, { priority: 'viewport', viewportGeneration: firstGeneration });

    const nextGeneration = scheduler.beginViewportRegionLoad(['shared']);
    const retained = scheduler.scheduleRegionLoad('shared', async () => {
      started += 1;
      return true;
    }, { priority: 'viewport', viewportGeneration: nextGeneration });
    release.resolve();

    await expect(Promise.all([first, retained])).resolves.toEqual([true, true]);
    expect(started).toBe(1);
  });

  it('admits required work ahead of queued viewport loads', async () => {
    const scheduler = createDocumentRegionLoadScheduler();
    const keys = Array.from({ length: 16 }, (_, index) => `viewport-${index}`);
    const generation = scheduler.beginViewportRegionLoad(keys);
    const releases: Array<() => void> = [];
    let completed = 0;
    const viewportLoads = keys.map((key) => scheduler.scheduleRegionLoad(key, async () => {
      await new Promise<void>((resolve) => releases.push(resolve));
      completed += 1;
      return true;
    }, { priority: 'viewport', viewportGeneration: generation }));
    await waitFor(() => releases.length === 4);

    let requiredStarted = false;
    const required = scheduler.scheduleRegionLoad('required', async () => {
      requiredStarted = true;
      return true;
    });
    releases.shift()?.();
    await waitFor(() => requiredStarted);

    expect(await required).toBe(true);
    while (completed < 15) {
      await waitFor(() => releases.length > 0);
      const previousCompleted = completed;
      releases.splice(0).forEach((release) => release());
      await waitFor(() => completed > previousCompleted);
    }
    const viewportResults = await Promise.all(viewportLoads);
    expect(viewportResults.filter(Boolean)).toHaveLength(15);
  });

  it('invalidates queued work and waits for active loads to drain', async () => {
    const scheduler = createDocumentRegionLoadScheduler();
    const active = deferred<void>();
    let queuedStarted = false;
    const activeLoads = Array.from({ length: 4 }, (_, index) =>
      scheduler.scheduleRegionLoad(`active-${index}`, async () => {
        await active.promise;
        return true;
      })
    );
    const queued = scheduler.scheduleRegionLoad('queued', async () => {
      queuedStarted = true;
      return true;
    });
    scheduler.reset();

    let idle = false;
    const drained = scheduler.waitForIdle().then(() => {
      idle = true;
    });
    await Promise.resolve();
    expect(idle).toBe(false);
    expect(await queued).toBe(false);
    expect(queuedStarted).toBe(false);

    active.resolve();
    expect(await Promise.all(activeLoads)).toEqual([false, false, false, false]);
    await drained;
    expect(idle).toBe(true);
  });

  it('ends active observations on disposal without releasing unresolved requests', async () => {
    const cancellation = createOperationCancellationSource();
    const scheduler = createDocumentRegionLoadScheduler(cancellation.signal);
    const started = deferred<void>();
    const load = scheduler.scheduleRegionLoad('unresolved', async () => {
      started.resolve();
      return new Promise(() => undefined);
    });

    await started.promise;
    cancellation.cancel();

    await expect(load).resolves.toBe(false);
    await expect(scheduler.waitForIdle()).resolves.toBeUndefined();
    await expect(scheduler.scheduleRegionLoad('late', async () => true)).resolves.toBe(false);
  });

});

async function waitFor(predicate: () => boolean) {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    if (predicate()) return;
    await Promise.resolve();
  }
  throw new Error('condition was not reached');
}
