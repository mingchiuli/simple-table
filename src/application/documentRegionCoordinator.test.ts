import { describe, expect, it, vi } from 'vitest';

import { createDocumentRegionCoordinator } from '@/application/documentRegionCoordinator';
import { defaultRichProjection } from '@/types';
import type {
  DocumentRegionProjection,
  EditorCommandContext,
  LoadedSheetSlot,
  SheetRegion,
  SheetRegionBlock,
} from '@/types';

const context: EditorCommandContext = { documentId: '1', baseRevision: '0' };

describe('documentRegionCoordinator', () => {
  it('loads and commits a bounded region through its narrow document port', async () => {
    const committed: SheetRegionBlock[] = [];
    const region = sheetRegion();
    const coordinator = createDocumentRegionCoordinator({
      activateResidentSheet: () => true,
      loadedSheet: () => loadedSheet(),
      currentCommandContext: () => context,
      matchesCommandContext: (candidate) => candidate === context,
      pinRegionBlocksForLoad: () => undefined,
      touchLoadedRegion: () => false,
      commitLoadedRegionBlocks: (_context, _region, blocks) => {
        committed.push(...blocks);
        return true;
      },
      isSheetRegionLoaded: () => committed.length > 0,
    });

    const loaded = await coordinator.ensureSheetRegionLoaded(region, async () => response(region));

    expect(loaded).toBe(true);
    expect(committed).toHaveLength(1);
    expect(committed[0]?.region).toEqual(region);
  });

  it('invalidates an in-flight region request when the document session resets', async () => {
    const region = sheetRegion();
    let resolveResponse!: (response: DocumentRegionProjection) => void;
    const coordinator = createDocumentRegionCoordinator({
      activateResidentSheet: () => true,
      loadedSheet: () => loadedSheet(),
      currentCommandContext: () => context,
      matchesCommandContext: () => true,
      pinRegionBlocksForLoad: () => undefined,
      touchLoadedRegion: () => false,
      commitLoadedRegionBlocks: () => true,
      isSheetRegionLoaded: () => false,
    });
    const loading = coordinator.ensureSheetRegionLoaded(
      region,
      () => new Promise((resolve) => { resolveResponse = resolve; }),
    );

    coordinator.reset();
    resolveResponse(response(region));

    await expect(loading).resolves.toBe(false);
  });

  it('rejects a runtime region envelope for another document revision', async () => {
    const region = sheetRegion();
    const commitLoadedRegionBlocks = vi.fn(() => true);
    const coordinator = createDocumentRegionCoordinator({
      activateResidentSheet: () => true,
      loadedSheet: () => loadedSheet(),
      currentCommandContext: () => context,
      matchesCommandContext: () => true,
      pinRegionBlocksForLoad: () => undefined,
      touchLoadedRegion: () => false,
      commitLoadedRegionBlocks,
      isSheetRegionLoaded: () => false,
    });

    const mismatched = { ...response(region), revision: '1' as const };

    await expect(
      coordinator.ensureSheetRegionLoaded(region, async () => mismatched),
    ).resolves.toBe(false);
    expect(commitLoadedRegionBlocks).not.toHaveBeenCalled();
  });
});

function sheetRegion(): SheetRegion {
  return { sheetIndex: 0, rowStart: 0, rowEnd: 1, colStart: 0, colEnd: 1 };
}

function loadedSheet(): LoadedSheetSlot {
  return {
    state: 'loaded',
    name: 'Sheet1',
    extent: { rowCount: 1, columnCount: 1 },
    layout: { columnWidths: {}, rowHeights: {} },
    blocks: [],
    metadata: { merges: [], rich: defaultRichProjection() },
  };
}

function response(region: SheetRegion): DocumentRegionProjection {
  return {
    documentId: context.documentId,
    revision: context.baseRevision,
    projection: {
      region,
      cells: [],
      mergeAnchorCells: [],
      metadata: { merges: [], cellFormats: {}, cellStyles: {} },
      estimatedBytes: 1,
    },
  };
}
