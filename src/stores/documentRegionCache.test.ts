import { describe, expect, it } from 'vitest';
import {
  oldestEvictableRegionBlock,
  pinRegionBlocks,
  replacePinnedRegionBlock,
  scheduleRegionLoad,
  touchRegionBlock,
} from '@/stores/documentRegionCache';

describe('documentRegionCache', () => {
  it('runs at most four region requests concurrently', async () => {
    const owner = {};
    let active = 0;
    let peak = 0;
    const releases: Array<() => void> = [];
    const loads = Array.from({ length: 6 }, (_, index) =>
      scheduleRegionLoad(owner, `block-${index}`, async () => {
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

  it('evicts the least recently used unpinned block', () => {
    const owner = {};
    touchRegionBlock(owner, 'old');
    touchRegionBlock(owner, 'visible');
    touchRegionBlock(owner, 'recent');
    pinRegionBlocks(owner, ['visible']);

    expect(oldestEvictableRegionBlock(owner, new Set(['old', 'visible', 'recent']))).toBe('old');
    touchRegionBlock(owner, 'old');
    expect(oldestEvictableRegionBlock(owner, new Set(['old', 'visible', 'recent']))).toBe('recent');
  });

  it('replaces one parent pin without dropping other visible blocks', () => {
    const owner = {};
    pinRegionBlocks(owner, ['parent', 'other']);

    replacePinnedRegionBlock(owner, 'parent', ['child-a', 'child-b']);

    expect(oldestEvictableRegionBlock(
      owner,
      new Set(['other', 'child-a', 'child-b'])
    )).toBeUndefined();
  });
});

async function waitFor(predicate: () => boolean) {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    if (predicate()) return;
    await Promise.resolve();
  }
  throw new Error('condition was not reached');
}
