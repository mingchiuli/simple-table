import { describe, expect, it } from 'vitest';
import {
  createDocumentRegionLoadScheduler,
} from '@/application/documentRegionLoadScheduler';
import {
  oldestEvictableRegionBlock,
  pinRegionBlocks,
  replacePinnedRegionBlock,
  touchRegionBlock,
} from '@/stores/documentRegionState';

describe('documentRegionCache', () => {
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
    expect(oldResults.filter(Boolean)).toHaveLength(4);
    expect(started).toBe(5);
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

  it('evicts the least recently used unpinned block', () => {
    const owner = { regionLru: [] as string[], pinnedRegionBlocks: [] as string[] };
    touchRegionBlock(owner, 'old');
    touchRegionBlock(owner, 'visible');
    touchRegionBlock(owner, 'recent');
    pinRegionBlocks(owner, ['visible']);

    expect(oldestEvictableRegionBlock(owner, new Set(['old', 'visible', 'recent']))).toBe('old');
    touchRegionBlock(owner, 'old');
    expect(oldestEvictableRegionBlock(owner, new Set(['old', 'visible', 'recent']))).toBe('recent');
  });

  it('replaces one parent pin without dropping other visible blocks', () => {
    const owner = { regionLru: [] as string[], pinnedRegionBlocks: [] as string[] };
    pinRegionBlocks(owner, ['parent', 'other']);

    replacePinnedRegionBlock(owner, 'parent', ['child-a', 'child-b']);

    expect(oldestEvictableRegionBlock(
      owner,
      new Set(['other', 'child-a', 'child-b'])
    )).toBeUndefined();
  });

  it('does not repin a completed tile from an obsolete viewport', () => {
    const owner = { regionLru: [] as string[], pinnedRegionBlocks: [] as string[] };
    pinRegionBlocks(owner, ['current']);

    replacePinnedRegionBlock(owner, 'obsolete', ['obsolete-child']);

    expect(oldestEvictableRegionBlock(
      owner,
      new Set(['current', 'obsolete-child'])
    )).toBe('obsolete-child');
  });
});

async function waitFor(predicate: () => boolean) {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    if (predicate()) return;
    await Promise.resolve();
  }
  throw new Error('condition was not reached');
}
