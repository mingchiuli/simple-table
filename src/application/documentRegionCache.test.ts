import { describe, expect, it } from 'vitest';

import { createDocumentRegionCache } from '@/application/documentRegionCache';
import { createLoadedSheetSlot, regionKey } from '@/projection/documentProjection';
import type {
  DocumentProjection,
  EditorCommandContext,
  SheetRegion,
  SheetRegionBlock,
} from '@/types/documentRuntime';

describe('documentRegionCache', () => {
  it('evicts resident sheets without evicting the protected sheet', () => {
    const owner = createDocumentOwner({
      path: '/tmp/book.xlsx',
      fileName: 'book.xlsx',
      sheets: Array.from({ length: 5 }, (_, index) => loadedSheet(`Sheet ${index + 1}`, [])),
    });
    const cache = createDocumentRegionCache(owner.port);

    cache.reconcileProjection(0);

    expect(owner.data.sheets[0]?.state).toBe('loaded');
    expect(owner.data.sheets[1]?.state).toBe('unloaded');
    expect(cache.captureSnapshot().residentSheetOrder).toEqual([0, 2, 3, 4]);
  });

  it('keeps pinned blocks while enforcing the per-sheet LRU budget', () => {
    const blocks = Array.from({ length: 9 }, (_, index) => regionBlock(index));
    const owner = createDocumentOwner({
      path: '/tmp/book.xlsx',
      fileName: 'book.xlsx',
      sheets: [loadedSheet('Sheet 1', blocks)],
    });
    const cache = createDocumentRegionCache(owner.port);
    cache.pinRegionBlocksForLoad([blocks[0]!.region]);

    cache.reconcileProjection(0);

    const slot = owner.data.sheets[0];
    expect(slot?.state).toBe('loaded');
    if (slot?.state !== 'loaded') throw new Error('expected loaded sheet');
    expect(slot.blocks).toHaveLength(8);
    expect(slot.blocks.some((block) => block.key === blocks[0]!.key)).toBe(true);
    expect(slot.blocks.some((block) => block.key === blocks[1]!.key)).toBe(false);
  });

  it('rejects a block commit after the command context changes', () => {
    const owner = createDocumentOwner({
      path: '/tmp/book.xlsx',
      fileName: 'book.xlsx',
      sheets: [loadedSheet('Sheet 1', [])],
    });
    const cache = createDocumentRegionCache(owner.port);

    expect(cache.commitLoadedRegionBlocks(
      { documentId: '1', baseRevision: '1' },
      regionBlock(0).region,
      [regionBlock(0)],
    )).toBe(false);
    expect(owner.data.sheets[0]).toMatchObject({ state: 'loaded', blocks: [] });
  });
});

function createDocumentOwner(initial: DocumentProjection) {
  let data = initial;
  const context: EditorCommandContext = { documentId: '1', baseRevision: '0' };
  return {
    get data() { return data; },
    port: {
      get data() { return data; },
      manifestResidentBytes: 0,
      currentCommandContext: () => context,
      matchesCommandContext: (candidate: EditorCommandContext) =>
        candidate.documentId === context.documentId
        && candidate.baseRevision === context.baseRevision,
      replaceCachedProjection: (projection: DocumentProjection) => {
        data = projection;
      },
    },
  };
}

function loadedSheet(name: string, blocks: SheetRegionBlock[]) {
  return createLoadedSheetSlot(
    name,
    { rowCount: 2_000, columnCount: 32 },
    { columnWidths: {}, rowHeights: {} },
    blocks,
  );
}

function regionBlock(index: number): SheetRegionBlock {
  const region: SheetRegion = {
    sheetIndex: 0,
    rowStart: index * 128,
    rowEnd: (index + 1) * 128,
    colStart: 0,
    colEnd: 32,
  };
  return {
    key: regionKey(region),
    region,
    cells: {},
    mergeAnchorCells: {},
    metadata: { merges: [], cellFormats: {}, cellStyles: {} },
    wireBytes: 1,
    residentBytes: 1,
  };
}
