import type {
  DocumentRegionProjection,
  EditorCommandContext,
  SheetExtent,
  SheetRegion,
  SheetRegionBlock,
} from '@/types/documentRuntime';
import {
  MAX_REGION_RESPONSE_BYTES,
  SHEET_REGION_TILE_COLUMNS,
  SHEET_REGION_TILE_ROWS,
} from '@/protocol/editorResourcePolicy';
import { regionBlock, regionKey } from '@/projection/documentProjection';
import { isAppErrorCode } from '@/utils/appError';

const MAX_REGION_FRAGMENT_REQUESTS = 64;
const MAX_REGION_FRAGMENT_BYTES = 32 * 1024 * 1024;
const MAX_REGION_LOAD_DURATION_MS = 10_000;

type RegionProjectionFetcher = (
  context: EditorCommandContext,
  region: SheetRegion
) => Promise<DocumentRegionProjection>;

type RegionLoadBudget = {
  remainingRequests: number;
  loadedBytes: number;
  deadline: number;
};

export function tileRegions(region: SheetRegion, extent: SheetExtent): SheetRegion[] {
  const rowStart = Math.max(0, Math.min(region.rowStart, extent.rowCount));
  const rowEnd = Math.max(rowStart, Math.min(region.rowEnd, extent.rowCount));
  const colStart = Math.max(0, Math.min(region.colStart, extent.columnCount));
  const colEnd = Math.max(colStart, Math.min(region.colEnd, extent.columnCount));
  if (rowStart === rowEnd || colStart === colEnd) return [];
  const tiles: SheetRegion[] = [];
  for (
    let row = Math.floor(rowStart / SHEET_REGION_TILE_ROWS) * SHEET_REGION_TILE_ROWS;
    row < rowEnd;
    row += SHEET_REGION_TILE_ROWS
  ) {
    for (
      let col = Math.floor(colStart / SHEET_REGION_TILE_COLUMNS) * SHEET_REGION_TILE_COLUMNS;
      col < colEnd;
      col += SHEET_REGION_TILE_COLUMNS
    ) {
      tiles.push({
        sheetIndex: region.sheetIndex,
        rowStart: row,
        rowEnd: Math.min(row + SHEET_REGION_TILE_ROWS, extent.rowCount),
        colStart: col,
        colEnd: Math.min(col + SHEET_REGION_TILE_COLUMNS, extent.columnCount),
      });
    }
  }
  return tiles;
}

export function loadRegionBlocks(
  context: EditorCommandContext,
  region: SheetRegion,
  fetchProjection: RegionProjectionFetcher,
  isCurrent: () => boolean
): Promise<SheetRegionBlock[]> {
  return fetchRegionBlocks(
    context,
    region,
    fetchProjection,
    {
      remainingRequests: MAX_REGION_FRAGMENT_REQUESTS,
      loadedBytes: 0,
      deadline: Date.now() + MAX_REGION_LOAD_DURATION_MS,
    },
    isCurrent
  );
}

async function fetchRegionBlocks(
  context: EditorCommandContext,
  region: SheetRegion,
  fetchProjection: RegionProjectionFetcher,
  budget: RegionLoadBudget,
  isCurrent: () => boolean
): Promise<SheetRegionBlock[]> {
  ensureRegionLoadCanContinue(budget, isCurrent);
  budget.remainingRequests -= 1;
  try {
    const response = await fetchProjection(context, region);
    ensureRegionLoadCanContinue(budget, isCurrent);
    if (regionKey(response.projection.region) !== regionKey(region)) return [];
    if (
      response.documentId !== context.documentId
      || response.revision !== context.baseRevision
    ) return [];
    const block = regionBlock(response.projection);
    if (block.estimatedBytes > MAX_REGION_RESPONSE_BYTES) return [];
    budget.loadedBytes += block.estimatedBytes;
    if (budget.loadedBytes > MAX_REGION_FRAGMENT_BYTES) {
      throw new RegionLoadLimitError(
        `Region fragments exceed the ${MAX_REGION_FRAGMENT_BYTES} byte load budget`
      );
    }
    return [block];
  } catch (error) {
    if (!isAppErrorCode(error, 'region_response_too_large')) throw error;
    const split = splitRegion(region);
    if (!split) throw error;
    const first = await fetchRegionBlocks(
      context,
      split[0],
      fetchProjection,
      budget,
      isCurrent
    );
    const second = await fetchRegionBlocks(
      context,
      split[1],
      fetchProjection,
      budget,
      isCurrent
    );
    return [...first, ...second];
  }
}

function ensureRegionLoadCanContinue(budget: RegionLoadBudget, isCurrent: () => boolean) {
  if (!isCurrent()) {
    throw new RegionLoadCancelledError();
  }
  if (Date.now() > budget.deadline) {
    throw new RegionLoadLimitError(
      `Region load exceeded the ${MAX_REGION_LOAD_DURATION_MS} ms deadline`
    );
  }
  if (budget.remainingRequests <= 0) {
    throw new RegionLoadLimitError(
      `Region load requires more than ${MAX_REGION_FRAGMENT_REQUESTS} fragment requests`
    );
  }
}

function splitRegion(region: SheetRegion): [SheetRegion, SheetRegion] | null {
  const rows = region.rowEnd - region.rowStart;
  const columns = region.colEnd - region.colStart;
  if (rows <= 1 && columns <= 1) return null;
  if (rows >= columns && rows > 1) {
    const middle = region.rowStart + Math.floor(rows / 2);
    return [
      { ...region, rowEnd: middle },
      { ...region, rowStart: middle },
    ];
  }
  const middle = region.colStart + Math.floor(columns / 2);
  return [
    { ...region, colEnd: middle },
    { ...region, colStart: middle },
  ];
}

class RegionLoadLimitError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'RegionLoadLimitError';
  }
}

class RegionLoadCancelledError extends Error {
  constructor() {
    super('Region load was cancelled');
    this.name = 'RegionLoadCancelledError';
  }
}
