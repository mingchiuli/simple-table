import type {
  CellValue,
  DocumentManifest,
  DocumentProjection,
  EditorPatch,
  LoadedSheetSlot,
  ReadOnlyRichProjection,
  SheetExtent,
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
      return {
        state: 'loaded',
        name: sheet.name,
        extent: sheet.extent,
        blocks: [regionBlock(initialRegion)],
      };
    }
    return { state: 'unloaded', name: sheet.name, extent: sheet.extent };
  });
  return { path: manifest.path, fileName: manifest.fileName, sheets };
}

export function applyProjectionPatches(
  data: DocumentProjection | null,
  patches: EditorPatch[] | undefined,
  responseExtents?: SheetExtent[]
): ProjectionPatchResult {
  if (!data) return { data, resyncRequired: false };
  let sheets = [...data.sheets];
  let resyncRequired = false;

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
        sheets.splice(sheetIndex, 0, {
          state: 'loaded',
          name: sheet.name,
          extent: sheet.extent,
          blocks: [],
        });
        break;
      }
      case 'SheetDeleted':
        sheets.splice(patch.data.patch.sheetIndex, 1);
        break;
      case 'SheetsReplaced': {
        const { startIndex, sheets: replacements } = patch.data.patch;
        sheets = [
          ...sheets.slice(0, startIndex),
          ...replacements.map((sheet): SheetSlot => ({
            state: 'unloaded',
            name: sheet.name,
            extent: sheet.extent,
          })),
        ];
        break;
      }
      case 'SheetInvalidated':
      case 'RowInserted':
      case 'RowDeleted':
      case 'ColumnInserted':
      case 'ColumnDeleted': {
        const sheetIndex = patch.data.patch.sheetIndex;
        const current = sheets[sheetIndex];
        if (current) sheets[sheetIndex] = invalidateLoadedSheet(current);
        break;
      }
      case 'ResyncRequired':
        resyncRequired = true;
        break;
      default:
        assertNever(patch);
    }
  }

  if (responseExtents) {
    sheets = sheets.map((sheet, index) => ({
      ...sheet,
      extent: responseExtents[index] ?? sheet.extent,
    }));
  }
  sheets = reindexSheetBlocks(sheets);
  return { data: { ...data, sheets }, resyncRequired };
}

export function regionBlock(response: SheetRegionProjectionResponse): SheetRegionBlock {
  return {
    key: regionKey(response.region),
    region: response.region,
    cells: new Map(response.cells.map((cell) => [cellKey(cell.row, cell.col), cell.value])),
    metadata: normalizeMetadata(response.metadata),
  };
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
  }
  return undefined;
}

export function isCellLoaded(slot: SheetSlot | null | undefined, row: number, col: number): boolean {
  return slot?.state === 'loaded'
    && slot.blocks.some((block) => containsCell(block.region, row, col));
}

export function isRegionLoaded(slot: SheetSlot | null | undefined, region: SheetRegion): boolean {
  return slot?.state === 'loaded'
    && slot.blocks.some((block) => containsRegion(block.region, region));
}

export function loadedSheetMetadata(slot: LoadedSheetSlot): SheetRegionMetadata {
  const rich = defaultRichProjection();
  const merges = new Map<string, NonNullable<SheetRegionMetadata['merges']>[number]>();
  const columnWidths: Record<number, number> = {};
  const rowHeights: Record<number, number> = {};

  for (const block of slot.blocks) {
    for (const merge of block.metadata.merges ?? []) {
      merges.set(`${merge.startRow}:${merge.startCol}:${merge.endRow}:${merge.endCol}`, merge);
    }
    Object.assign(columnWidths, block.metadata.columnWidths ?? {});
    Object.assign(rowHeights, block.metadata.rowHeights ?? {});
    mergeRichProjection(rich, block.metadata.rich);
  }
  return {
    merges: [...merges.values()],
    columnWidths,
    rowHeights,
    rich,
  };
}

export function regionKey(region: SheetRegion): string {
  return `${region.sheetIndex}:${region.rowStart}:${region.rowEnd}:${region.colStart}:${region.colEnd}`;
}

function applyCellChanges(
  sheets: SheetSlot[],
  changes: Extract<EditorPatch, { type: 'Cells' }>['data']['changes']
): SheetSlot[] {
  const next = [...sheets];
  for (const change of changes) {
    const slot = next[change.sheetIndex];
    if (!slot || slot.state !== 'loaded') continue;
    let changed = false;
    const blocks = slot.blocks.map((block) => {
      if (!containsCell(block.region, change.row, change.col)) return block;
      changed = true;
      const cells = new Map(block.cells);
      cells.set(cellKey(change.row, change.col), change.value);
      return { ...block, cells };
    });
    if (changed) next[change.sheetIndex] = { ...slot, blocks };
  }
  return next;
}

function applyLayoutPatch(
  sheets: SheetSlot[],
  patch: Extract<EditorPatch, { type: 'Layout' }>['data']['patch']
): SheetSlot[] {
  const slot = sheets[patch.sheetIndex];
  if (!slot || slot.state !== 'loaded') return sheets;
  const blocks = slot.blocks.map((block) => {
    const columnWidths = { ...(block.metadata.columnWidths ?? {}) };
    const rowHeights = { ...(block.metadata.rowHeights ?? {}) };
    for (const [key, value] of Object.entries(patch.columnWidths ?? {})) {
      const index = Number(key);
      if (index < block.region.colStart || index >= block.region.colEnd) continue;
      if (value == null) delete columnWidths[index];
      else columnWidths[index] = value;
    }
    for (const [key, value] of Object.entries(patch.rowHeights ?? {})) {
      const index = Number(key);
      if (index < block.region.rowStart || index >= block.region.rowEnd) continue;
      if (value == null) delete rowHeights[index];
      else rowHeights[index] = value;
    }
    return { ...block, metadata: { ...block.metadata, columnWidths, rowHeights } };
  });
  const next = [...sheets];
  next[patch.sheetIndex] = { ...slot, blocks };
  return next;
}

function invalidateLoadedSheet(slot: SheetSlot): SheetSlot {
  return slot.state === 'loaded' ? { ...slot, blocks: [] } : slot;
}

function reindexSheetBlocks(sheets: SheetSlot[]): SheetSlot[] {
  return sheets.map((slot, sheetIndex) => {
    if (slot.state !== 'loaded') return slot;
    return {
      ...slot,
      blocks: slot.blocks.map((block) => {
        const region = { ...block.region, sheetIndex };
        return { ...block, region, key: regionKey(region) };
      }),
    };
  });
}

function normalizeMetadata(metadata: SheetRegionMetadata): SheetRegionMetadata {
  return {
    merges: metadata.merges ?? [],
    columnWidths: metadata.columnWidths ?? {},
    rowHeights: metadata.rowHeights ?? {},
    rich: {
      ...defaultRichProjection(),
      ...metadata.rich,
      cellFormats: metadata.rich.cellFormats ?? {},
      cellStyles: metadata.rich.cellStyles ?? {},
      hiddenRows: metadata.rich.hiddenRows ?? [],
      hiddenColumns: metadata.rich.hiddenColumns ?? [],
      hyperlinks: metadata.rich.hyperlinks ?? {},
      drawings: metadata.rich.drawings ?? [],
    },
  };
}

function mergeRichProjection(target: ReadOnlyRichProjection, source: ReadOnlyRichProjection) {
  target.cellFormats = { ...(target.cellFormats ?? {}), ...(source.cellFormats ?? {}) };
  target.cellStyles = { ...(target.cellStyles ?? {}), ...(source.cellStyles ?? {}) };
  target.hyperlinks = { ...(target.hyperlinks ?? {}), ...(source.hyperlinks ?? {}) };
  target.hiddenRows = [...new Set([...(target.hiddenRows ?? []), ...(source.hiddenRows ?? [])])];
  target.hiddenColumns = [
    ...new Set([...(target.hiddenColumns ?? []), ...(source.hiddenColumns ?? [])]),
  ];
  const drawings = new Map<string, NonNullable<ReadOnlyRichProjection['drawings']>[number]>();
  for (const drawing of [...(target.drawings ?? []), ...(source.drawings ?? [])]) {
    drawings.set(
      `${drawing.kind}:${drawing.fromRow}:${drawing.fromCol}:${drawing.toRow}:${drawing.toCol}`,
      drawing
    );
  }
  target.drawings = [...drawings.values()];
  target.freezePane ??= source.freezePane;
  target.hasMoreDrawings ||= source.hasMoreDrawings;
  target.hasStyleMetadata ||= source.hasStyleMetadata;
  target.hasHyperlinks ||= source.hasHyperlinks;
  target.hasFreezePane ||= source.hasFreezePane;
}

function containsCell(region: SheetRegion, row: number, col: number): boolean {
  return row >= region.rowStart && row < region.rowEnd
    && col >= region.colStart && col < region.colEnd;
}

function containsRegion(loaded: SheetRegion, requested: SheetRegion): boolean {
  return loaded.sheetIndex === requested.sheetIndex
    && loaded.rowStart <= requested.rowStart
    && loaded.rowEnd >= requested.rowEnd
    && loaded.colStart <= requested.colStart
    && loaded.colEnd >= requested.colEnd;
}

function cellKey(row: number, col: number): string {
  return `${row}:${col}`;
}

function assertNever(value: never): never {
  throw new Error(`Unsupported editor patch: ${JSON.stringify(value)}`);
}
