import type {
  CellValue,
  DocumentManifest,
  DocumentProjection,
  EditorPatch,
  LoadedSheetRegionMetadata,
  LoadedSheetSlot,
  SheetExtent,
  SheetLayoutProjection,
  SheetLayoutState,
  SheetRegion,
  SheetRegionBlock,
  SheetRegionMetadata,
  SheetRegionProjectionResponse,
  SheetSlot,
} from '@/types';
import { defaultRichProjection } from '@/types';

export type ProjectionPatchResult = {
  data: DocumentProjection | null;
  resyncRequired: boolean;
};

export function createDocumentProjection(
  manifest: DocumentManifest,
  initialRegion?: SheetRegionProjectionResponse
): DocumentProjection {
  const sheets: SheetSlot[] = manifest.sheets.map((sheet, index) => {
    if (initialRegion?.region.sheetIndex === index) {
      return createLoadedSheetSlot(
        sheet.name,
        sheet.extent,
        sheet.layout,
        [regionBlock(initialRegion)]
      );
    }
    return {
      state: 'unloaded',
      name: sheet.name,
      extent: sheet.extent,
      layout: normalizeSheetLayout(sheet.layout),
    };
  });
  return { path: manifest.path, fileName: manifest.fileName, sheets };
}

export function applyProjectionPatches(
  data: DocumentProjection | null,
  patches: EditorPatch[] | undefined,
  responseExtents?: SheetExtent[]
): ProjectionPatchResult {
  if (!data) return { data, resyncRequired: false };
  let sheets = data.sheets;
  let resyncRequired = false;
  let sheetIndexesChanged = false;

  for (const patch of patches ?? []) {
    switch (patch.type) {
      case 'Cells':
        sheets = applyCellChanges(sheets, patch.data.changes);
        break;
      case 'Layout':
        sheets = applyLayoutPatch(sheets, patch.data.patch);
        break;
      case 'SheetInserted': {
        const { sheetIndex, sheet } = patch.data.patch;
        sheets = [...sheets];
        sheets.splice(
          sheetIndex,
          0,
          createLoadedSheetSlot(sheet.name, sheet.extent, sheet.layout, [])
        );
        sheetIndexesChanged = true;
        break;
      }
      case 'SheetDeleted':
        sheets = [...sheets];
        sheets.splice(patch.data.patch.sheetIndex, 1);
        sheetIndexesChanged = true;
        break;
      case 'SheetsReplaced': {
        const { startIndex, sheets: replacements } = patch.data.patch;
        sheets = [
          ...sheets.slice(0, startIndex),
          ...replacements.map((sheet): SheetSlot => ({
            state: 'unloaded',
            name: sheet.name,
            extent: sheet.extent,
            layout: normalizeSheetLayout(sheet.layout),
          })),
        ];
        sheetIndexesChanged = true;
        break;
      }
      case 'SheetInvalidated': {
        const sheetIndex = patch.data.patch.sheetIndex;
        const current = sheets[sheetIndex];
        if (current) {
          const invalidated = invalidateLoadedSheet(current);
          if (invalidated !== current) {
            sheets = [...sheets];
            sheets[sheetIndex] = invalidated;
          }
        }
        break;
      }
      case 'RowInserted':
        sheets = applyAxisStructurePatch(
          sheets,
          patch.data.patch.sheetIndex,
          'row',
          'insert',
          patch.data.patch.rowIndex,
          patch.data.patch.count
        );
        break;
      case 'RowDeleted':
        sheets = applyAxisStructurePatch(
          sheets,
          patch.data.patch.sheetIndex,
          'row',
          'delete',
          patch.data.patch.rowIndex,
          patch.data.patch.count
        );
        break;
      case 'ColumnInserted':
        sheets = applyAxisStructurePatch(
          sheets,
          patch.data.patch.sheetIndex,
          'column',
          'insert',
          patch.data.patch.colIndex,
          patch.data.patch.count
        );
        break;
      case 'ColumnDeleted':
        sheets = applyAxisStructurePatch(
          sheets,
          patch.data.patch.sheetIndex,
          'column',
          'delete',
          patch.data.patch.colIndex,
          patch.data.patch.count
        );
        break;
      case 'ResyncRequired':
        resyncRequired = true;
        break;
      default:
        assertNever(patch);
    }
  }

  if (responseExtents) {
    sheets = applySheetExtents(sheets, responseExtents);
  }
  if (sheetIndexesChanged) sheets = reindexSheetBlocks(sheets);
  return {
    data: sheets === data.sheets ? data : { ...data, sheets },
    resyncRequired,
  };
}

export function regionBlock(response: SheetRegionProjectionResponse): SheetRegionBlock {
  return {
    key: regionKey(response.region),
    region: response.region,
    cells: new Map(response.cells.map((cell) => [cellKey(cell.row, cell.col), cell.value])),
    mergeAnchorCells: new Map(
      (response.mergeAnchorCells ?? []).map((cell) => [cellKey(cell.row, cell.col), cell.value])
    ),
    metadata: normalizeMetadata(response.metadata),
    estimatedBytes: response.estimatedBytes ?? estimateRegionBytes(response),
  };
}

function estimateRegionBytes(response: SheetRegionProjectionResponse): number {
  const metadata = response.metadata;
  return 512
    + response.cells.length * 256
    + (response.mergeAnchorCells?.length ?? 0) * 256
    + (metadata.merges?.length ?? 0) * 64
    + Object.keys(metadata.cellFormats ?? {}).length * 256
    + Object.keys(metadata.cellStyles ?? {}).length * 512;
}

export function sheetCell(
  slot: SheetSlot | null | undefined,
  row: number,
  col: number
): CellValue | undefined {
  if (!slot || slot.state !== 'loaded') return undefined;
  const key = cellKey(row, col);
  for (let index = slot.blocks.length - 1; index >= 0; index -= 1) {
    const block = slot.blocks[index];
    if (containsCell(block.region, row, col)) return block.cells.get(key);
    const anchor = block.mergeAnchorCells.get(key);
    if (anchor !== undefined) return anchor;
  }
  return undefined;
}

export function isCellLoaded(slot: SheetSlot | null | undefined, row: number, col: number): boolean {
  return slot?.state === 'loaded'
    && slot.blocks.some((block) =>
      containsCell(block.region, row, col) || block.mergeAnchorCells.has(cellKey(row, col))
    );
}

export function isRegionLoaded(slot: SheetSlot | null | undefined, region: SheetRegion): boolean {
  return regionCoveringBlockKeys(slot, region) !== null;
}

export function regionCoveringBlockKeys(
  slot: SheetSlot | null | undefined,
  region: SheetRegion
): string[] | null {
  if (!slot || slot.state !== 'loaded') return null;
  if (region.rowStart >= region.rowEnd || region.colStart >= region.colEnd) return [];
  const blocks = slot.blocks.filter((block) => block.region.sheetIndex === region.sheetIndex
    && block.region.rowStart < region.rowEnd
    && block.region.rowEnd > region.rowStart
    && block.region.colStart < region.colEnd
    && block.region.colEnd > region.colStart);
  const rowBoundaries = new Set([region.rowStart, region.rowEnd]);
  for (const block of blocks) {
    rowBoundaries.add(Math.max(region.rowStart, block.region.rowStart));
    rowBoundaries.add(Math.min(region.rowEnd, block.region.rowEnd));
  }
  const rows = [...rowBoundaries].sort((left, right) => left - right);
  const used = new Set<string>();
  for (let index = 0; index + 1 < rows.length; index += 1) {
    const rowStart = rows[index];
    const rowEnd = rows[index + 1];
    if (rowStart === rowEnd) continue;
    const intervals = blocks
      .filter((block) => block.region.rowStart <= rowStart && block.region.rowEnd >= rowEnd)
      .map((block) => ({
        start: Math.max(region.colStart, block.region.colStart),
        end: Math.min(region.colEnd, block.region.colEnd),
        key: block.key,
      }))
      .filter((interval) => interval.start < interval.end)
      .sort((left, right) => left.start - right.start || right.end - left.end);
    let coveredUntil = region.colStart;
    for (const interval of intervals) {
      if (interval.start > coveredUntil) break;
      if (interval.end <= coveredUntil) continue;
      coveredUntil = interval.end;
      used.add(interval.key);
      if (coveredUntil >= region.colEnd) break;
    }
    if (coveredUntil < region.colEnd) return null;
  }
  return [...used];
}

export function loadedSheetMetadata(slot: LoadedSheetSlot) {
  return {
    merges: slot.metadata.merges,
    columnWidths: slot.layout.columnWidths,
    rowHeights: slot.layout.rowHeights,
    rich: slot.metadata.rich,
  };
}

export function createLoadedSheetSlot(
  name: string,
  extent: SheetExtent,
  layout: SheetLayoutProjection | SheetLayoutState,
  blocks: LoadedSheetSlot['blocks']
): LoadedSheetSlot {
  return {
    state: 'loaded',
    name,
    extent,
    layout: normalizeSheetLayout(layout),
    blocks,
    metadata: aggregateLoadedSheetMetadata(blocks),
  };
}

export function replaceLoadedSheetBlocks(
  slot: LoadedSheetSlot,
  blocks: LoadedSheetSlot['blocks']
): LoadedSheetSlot {
  if (slot.blocks === blocks) return slot;
  return {
    ...slot,
    blocks,
    metadata: aggregateLoadedSheetMetadata(blocks),
  };
}

export function regionKey(region: SheetRegion): string {
  return `${region.sheetIndex}:${region.rowStart}:${region.rowEnd}:${region.colStart}:${region.colEnd}`;
}

function applyCellChanges(
  sheets: SheetSlot[],
  changes: Extract<EditorPatch, { type: 'Cells' }>['data']['changes']
): SheetSlot[] {
  const changesBySheet = new Map<number, typeof changes>();
  for (const change of changes) {
    const sheetChanges = changesBySheet.get(change.sheetIndex);
    if (sheetChanges) sheetChanges.push(change);
    else changesBySheet.set(change.sheetIndex, [change]);
  }

  let next = sheets;
  for (const [sheetIndex, sheetChanges] of changesBySheet) {
    const slot = sheets[sheetIndex];
    if (!slot || slot.state !== 'loaded') continue;
    let sheetChanged = false;
    const blocks = slot.blocks.map((block) => {
      let cells: typeof block.cells | null = null;
      let mergeAnchorCells: typeof block.mergeAnchorCells | null = null;
      for (const change of sheetChanges) {
        const key = cellKey(change.row, change.col);
        if (containsCell(block.region, change.row, change.col)) {
          cells ??= new Map(block.cells);
          cells.set(key, change.value);
        }
        if (block.mergeAnchorCells.has(key)) {
          mergeAnchorCells ??= new Map(block.mergeAnchorCells);
          mergeAnchorCells.set(key, change.value);
        }
      }
      if (!cells && !mergeAnchorCells) return block;
      sheetChanged = true;
      return {
        ...block,
        cells: cells ?? block.cells,
        mergeAnchorCells: mergeAnchorCells ?? block.mergeAnchorCells,
      };
    });
    if (!sheetChanged) continue;
    if (next === sheets) next = [...sheets];
    next[sheetIndex] = { ...slot, blocks };
  }
  return next;
}

function applyLayoutPatch(
  sheets: SheetSlot[],
  patch: Extract<EditorPatch, { type: 'Layout' }>['data']['patch']
): SheetSlot[] {
  const slot = sheets[patch.sheetIndex];
  if (!slot) return sheets;
  const columnWidths = applyLayoutValues(slot.layout.columnWidths, patch.columnWidths);
  const rowHeights = applyLayoutValues(slot.layout.rowHeights, patch.rowHeights);
  if (
    columnWidths === slot.layout.columnWidths
    && rowHeights === slot.layout.rowHeights
  ) return sheets;
  const next = [...sheets];
  next[patch.sheetIndex] = { ...slot, layout: { columnWidths, rowHeights } };
  return next;
}

function invalidateLoadedSheet(slot: SheetSlot): SheetSlot {
  return slot.state === 'loaded' ? replaceLoadedSheetBlocks(slot, []) : slot;
}

function applyAxisStructurePatch(
  sheets: SheetSlot[],
  sheetIndex: number,
  axis: 'row' | 'column',
  direction: 'insert' | 'delete',
  index: number,
  count: number
): SheetSlot[] {
  const slot = sheets[sheetIndex];
  if (!slot || count <= 0) return sheets;
  const next = [...sheets];
  const layout = {
    columnWidths: axis === 'column'
      ? shiftLayoutOverrides(slot.layout.columnWidths, direction, index, count)
      : slot.layout.columnWidths,
    rowHeights: axis === 'row'
      ? shiftLayoutOverrides(slot.layout.rowHeights, direction, index, count)
      : slot.layout.rowHeights,
  };
  const updated = { ...slot, layout };
  next[sheetIndex] = invalidateLoadedSheet(updated);
  return next;
}

function shiftLayoutOverrides(
  values: Record<number, number>,
  direction: 'insert' | 'delete',
  index: number,
  count: number
): Record<number, number> {
  const shifted: Record<number, number> = {};
  const deletedEnd = index + count;
  for (const [key, value] of Object.entries(values)) {
    const current = Number(key);
    if (direction === 'insert') {
      shifted[current >= index ? current + count : current] = value;
    } else if (current < index) {
      shifted[current] = value;
    } else if (current >= deletedEnd) {
      shifted[current - count] = value;
    }
  }
  return shifted;
}

function reindexSheetBlocks(sheets: SheetSlot[]): SheetSlot[] {
  let next = sheets;
  for (const [sheetIndex, slot] of sheets.entries()) {
    if (slot.state !== 'loaded') continue;
    const changed = slot.blocks.some((block) => block.region.sheetIndex !== sheetIndex);
    if (!changed) continue;
    if (next === sheets) next = [...sheets];
    next[sheetIndex] = {
      ...slot,
      blocks: slot.blocks.map((block) => {
        if (block.region.sheetIndex === sheetIndex) return block;
        const region = { ...block.region, sheetIndex };
        return { ...block, region, key: regionKey(region) };
      }),
    };
  }
  return next;
}

function applySheetExtents(sheets: SheetSlot[], extents: SheetExtent[]): SheetSlot[] {
  let next = sheets;
  for (const [index, sheet] of sheets.entries()) {
    const extent = extents[index];
    if (
      !extent
      || (
        extent.rowCount === sheet.extent.rowCount
        && extent.columnCount === sheet.extent.columnCount
      )
    ) continue;
    if (next === sheets) next = [...sheets];
    next[index] = { ...sheet, extent };
  }
  return next;
}

function normalizeMetadata(metadata: SheetRegionMetadata): SheetRegionMetadata {
  return {
    merges: metadata.merges ?? [],
    cellFormats: metadata.cellFormats ?? {},
    cellStyles: metadata.cellStyles ?? {},
  };
}

function normalizeSheetLayout(
  layout: SheetLayoutProjection | SheetLayoutState
): SheetLayoutState {
  return {
    columnWidths: layout.columnWidths ?? {},
    rowHeights: layout.rowHeights ?? {},
  };
}

function aggregateLoadedSheetMetadata(
  blocks: LoadedSheetSlot['blocks']
): LoadedSheetRegionMetadata {
  const rich = {
    ...defaultRichProjection(),
    cellFormats: {},
    cellStyles: {},
  };
  const merges = new Map<string, NonNullable<SheetRegionMetadata['merges']>[number]>();

  for (const block of blocks) {
    for (const merge of block.metadata.merges ?? []) {
      merges.set(`${merge.startRow}:${merge.startCol}:${merge.endRow}:${merge.endCol}`, merge);
    }
    Object.assign(rich.cellFormats, block.metadata.cellFormats ?? {});
    Object.assign(rich.cellStyles, block.metadata.cellStyles ?? {});
  }
  return { merges: [...merges.values()], rich };
}

function applyLayoutValues(
  source: Record<number, number>,
  updates: Record<number, number | null> | undefined
): Record<number, number> {
  let target = source;
  for (const [key, value] of Object.entries(updates ?? {})) {
    const index = Number(key);
    const hasValue = Object.prototype.hasOwnProperty.call(source, index);
    if (value == null) {
      if (!hasValue) continue;
      if (target === source) target = { ...source };
      delete target[index];
    } else if (!hasValue || source[index] !== value) {
      if (target === source) target = { ...source };
      target[index] = value;
    }
  }
  return target;
}

function containsCell(region: SheetRegion, row: number, col: number): boolean {
  return row >= region.rowStart && row < region.rowEnd
    && col >= region.colStart && col < region.colEnd;
}

function cellKey(row: number, col: number): string {
  return `${row}:${col}`;
}

function assertNever(value: never): never {
  throw new Error(`Unsupported editor patch: ${JSON.stringify(value)}`);
}
