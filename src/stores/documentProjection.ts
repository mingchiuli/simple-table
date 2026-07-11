import { applyDocumentPatches } from '@/stores/documentPatches';
import { calculateSheetExtent } from '@/table-geometry/sheetExtent';
import type {
  DocumentProjection,
  EditorPatch,
  FileData,
  SheetData,
  SheetExtent,
  SheetRegion,
  SheetSlot,
} from '@/types';

export type ProjectionPatchResult = {
  data: DocumentProjection | null;
  resyncRequired: boolean;
};

export function createDocumentProjection(
  fileData: FileData,
  extents?: SheetExtent[],
  loadedSheetIndexes?: number[],
  loadedRegions?: SheetRegion[]
): DocumentProjection {
  const loaded = new Set(
    loadedSheetIndexes ?? fileData.sheets.map((_, index) => index)
  );
  return {
    path: fileData.path,
    fileName: fileData.fileName,
    sheets: fileData.sheets.map((sheet, index) => {
      const extent = extents?.[index] ?? extentOf(sheet);
      const regions = loadedRegions?.filter((region) => region.sheetIndex === index)
        ?? [fullSheetRegion(index, extent)];
      return sheetSlot(sheet, extent, loaded.has(index), regions);
    }),
  };
}

export function fullFileData(data: DocumentProjection): FileData {
  return {
    path: data.path,
    fileName: data.fileName,
    sheets: data.sheets.map((slot) =>
      slot.state === 'loaded' ? slot.data : unloadedPlaceholder(slot.name)
    ),
  };
}

export function applyProjectionPatches(
  data: DocumentProjection | null,
  patches: EditorPatch[] | undefined,
  responseExtents?: SheetExtent[]
): ProjectionPatchResult {
  if (!data) return { data, resyncRequired: false };
  const loadedIndexes = data.sheets
    .map((slot, index) => slot.state === 'loaded' ? index : -1)
    .filter((index) => index >= 0);
  const result = applyDocumentPatches(fullFileData(data), patches);
  if (!result.data) return { data: null, resyncRequired: result.resyncRequired };
  const nextLoaded = updateLoadedSheetIndexes(loadedIndexes, patches);
  return {
    data: createDocumentProjection(
      result.data,
      responseExtents,
      nextLoaded,
      preservedRegions(data, patches)
    ),
    resyncRequired: result.resyncRequired,
  };
}

function sheetSlot(
  sheet: SheetData,
  extent: SheetExtent,
  loaded: boolean,
  regions: SheetRegion[]
): SheetSlot {
  if (!loaded) {
    return { state: 'unloaded', name: sheet.name, extent };
  }
  return { state: 'loaded', name: sheet.name, extent, data: sheet, regions };
}

function fullSheetRegion(sheetIndex: number, extent: SheetExtent): SheetRegion {
  return {
    sheetIndex,
    rowStart: 0,
    rowEnd: extent.rowCount,
    colStart: 0,
    colEnd: extent.columnCount,
  };
}

function preservedRegions(
  data: DocumentProjection,
  patches: EditorPatch[] | undefined
): SheetRegion[] | undefined {
  const onlyContentOrLayout = (patches ?? []).every((patch) =>
    patch.type === 'Cells' || patch.type === 'Layout'
  );
  if (!onlyContentOrLayout) return [];
  return data.sheets.flatMap((slot) => slot.state === 'loaded' ? slot.regions : []);
}

function unloadedPlaceholder(name: string): SheetData {
  return { name, rows: [], merges: [], rich: {
    cellFormats: {},
    cellStyles: {},
    hiddenRows: [],
    hiddenColumns: [],
    hyperlinks: {},
    drawings: [],
    hasMoreDrawings: false,
    hasStyleMetadata: false,
    hasHyperlinks: false,
    hasFreezePane: false,
  } };
}

function extentOf(sheet: SheetData): SheetExtent {
  return calculateSheetExtent(
    sheet.rows,
    sheet.merges,
    sheet.columnWidths,
    sheet.rowHeights,
    sheet.rich
  );
}

function updateLoadedSheetIndexes(
  loadedSheetIndexes: number[],
  patches: EditorPatch[] | undefined
): number[] {
  let loaded = new Set(loadedSheetIndexes);
  for (const patch of patches ?? []) {
    switch (patch.type) {
      case 'SheetInserted': {
        const inserted = patch.data.patch.sheetIndex;
        loaded = new Set(Array.from(loaded, (index) => index >= inserted ? index + 1 : index));
        loaded.add(inserted);
        break;
      }
      case 'SheetDeleted': {
        const deleted = patch.data.patch.sheetIndex;
        loaded = new Set(Array.from(loaded)
          .filter((index) => index !== deleted)
          .map((index) => index > deleted ? index - 1 : index));
        break;
      }
      case 'SheetUpdated':
        loaded.add(patch.data.patch.sheetIndex);
        break;
      case 'SheetsReplaced': {
        const { startIndex, sheets } = patch.data.patch;
        loaded = new Set(Array.from(loaded).filter((index) => index < startIndex));
        sheets.forEach((_, offset) => loaded.add(startIndex + offset));
        break;
      }
    }
  }
  return Array.from(loaded);
}
