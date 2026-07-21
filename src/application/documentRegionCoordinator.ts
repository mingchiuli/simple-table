import {
  createDocumentRegionLoadScheduler,
  type RegionLoadPriority,
} from '@/application/documentRegionLoadScheduler';
import {
  loadRegionBlocks,
  tileRegions,
  TILE_COLUMNS,
  TILE_ROWS,
} from '@/application/documentRegionRepository';
import type {
  EditorCommandContext,
  LoadedSheetSlot,
  SheetRegion,
  SheetRegionBlock,
} from '@/types/documentRuntime';
import type { SheetRegionProjectionResponse } from '@/types/protocol';

export type FetchRegionProjection = (
  context: EditorCommandContext,
  region: SheetRegion,
) => Promise<SheetRegionProjectionResponse>;

export type DocumentRegionPort = {
  activateResidentSheet(sheetIndex: number, protectedSheetIndex?: number): boolean;
  loadedSheet(sheetIndex: number): LoadedSheetSlot | null;
  currentCommandContext(): EditorCommandContext | null;
  matchesCommandContext(context: EditorCommandContext): boolean;
  pinRegionBlocksForLoad(regions: SheetRegion[]): void;
  touchLoadedRegion(region: SheetRegion): boolean;
  commitLoadedRegionBlocks(
    context: EditorCommandContext,
    region: SheetRegion,
    blocks: SheetRegionBlock[],
  ): boolean;
  isSheetRegionLoaded(region: SheetRegion): boolean;
};

export function createDocumentRegionCoordinator(document: DocumentRegionPort) {
  const loads = createDocumentRegionLoadScheduler();

  function reset() {
    loads.reset();
  }

  async function ensureSheetLoaded(
    sheetIndex: number,
    fetchProjection: FetchRegionProjection,
  ): Promise<boolean> {
    if (!document.activateResidentSheet(sheetIndex, sheetIndex)) return false;
    const slot = document.loadedSheet(sheetIndex);
    if (!slot) return false;
    if (slot.extent.rowCount === 0 || slot.extent.columnCount === 0) return true;
    return ensureSheetRegionLoaded({
      sheetIndex,
      rowStart: 0,
      rowEnd: Math.min(TILE_ROWS, slot.extent.rowCount),
      colStart: 0,
      colEnd: Math.min(TILE_COLUMNS, slot.extent.columnCount),
    }, fetchProjection);
  }

  async function ensureSheetRegionLoaded(
    region: SheetRegion,
    fetchProjection: FetchRegionProjection,
    options: { priority?: RegionLoadPriority } = {},
  ): Promise<boolean> {
    if (!document.activateResidentSheet(region.sheetIndex, region.sheetIndex)) return false;
    const slot = document.loadedSheet(region.sheetIndex);
    if (!slot) return false;
    const tiles = tileRegions(region, slot.extent);
    if (!tiles.length) return true;
    const context = document.currentCommandContext();
    if (!context) return false;
    const priority = options.priority ?? 'required';
    const viewportGeneration = priority === 'viewport'
      ? loads.beginViewportRegionLoad(tiles.map((tile) => regionLoadKey(context, tile)))
      : undefined;
    document.pinRegionBlocksForLoad(tiles);
    const results = await Promise.all(tiles.map((tile) => loadRegionBlock(
      context,
      tile,
      fetchProjection,
      { priority, viewportGeneration },
    )));
    return results.every(Boolean) && document.isSheetRegionLoaded(region);
  }

  function loadRegionBlock(
    context: EditorCommandContext,
    region: SheetRegion,
    fetchProjection: FetchRegionProjection,
    options: { priority: RegionLoadPriority; viewportGeneration?: number },
  ): Promise<boolean> {
    if (document.touchLoadedRegion(region)) return Promise.resolve(true);
    return loads.scheduleRegionLoad(
      regionLoadKey(context, region),
      async (isCurrent) => {
        let blocks: SheetRegionBlock[];
        try {
          blocks = await loadRegionBlocks(
            context,
            region,
            fetchProjection,
            () => isCurrent() && document.matchesCommandContext(context),
          );
        } catch (error) {
          if (!document.matchesCommandContext(context)) return false;
          throw error;
        }
        if (!isCurrent() || !document.matchesCommandContext(context)) return false;
        return document.commitLoadedRegionBlocks(context, region, blocks);
      },
      options,
    );
  }

  return { reset, ensureSheetLoaded, ensureSheetRegionLoaded };
}

function regionLoadKey(context: EditorCommandContext, region: SheetRegion) {
  return `${context.documentId}:${context.baseRevision}:${region.sheetIndex}:${region.rowStart}:${region.rowEnd}:${region.colStart}:${region.colEnd}`;
}
