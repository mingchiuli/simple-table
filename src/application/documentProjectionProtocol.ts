import type {
  CellValue as ProtocolCellValue,
  DocumentManifest as ProtocolDocumentManifest,
  EditorPatch as ProtocolEditorPatch,
  SheetExtent as ProtocolSheetExtent,
  SheetManifest as ProtocolSheetManifest,
  SheetRegionMetadata as ProtocolSheetRegionMetadata,
  SheetRegionProjectionResponse,
} from '@/types/protocol';
import type {
  CellValue,
  DocumentManifest,
  DocumentRegionProjection,
  EditorPatch,
  SheetExtent,
  SheetManifest,
  SheetRegionMetadata,
  SheetRegionProjection,
} from '@/types/documentRuntime';

export function runtimeDocumentManifest(
  manifest: ProtocolDocumentManifest,
): DocumentManifest {
  return {
    path: manifest.path,
    fileName: manifest.fileName,
    sheets: manifest.sheets.map(runtimeSheetManifest),
  };
}

export function runtimeRegionProjection(
  response: SheetRegionProjectionResponse,
): SheetRegionProjection {
  return {
    region: { ...response.region },
    cells: response.cells.map((change) => ({
      sheetIndex: change.sheetIndex,
      row: change.row,
      col: change.col,
      value: runtimeCellValue(change.value),
    })),
    mergeAnchorCells: (response.mergeAnchorCells ?? []).map((change) => ({
      sheetIndex: change.sheetIndex,
      row: change.row,
      col: change.col,
      value: runtimeCellValue(change.value),
    })),
    metadata: runtimeRegionMetadata(response.metadata),
    estimatedBytes: response.estimatedBytes,
  };
}

export function runtimeDocumentRegionProjection(
  response: SheetRegionProjectionResponse,
): DocumentRegionProjection {
  return {
    documentId: response.documentId,
    revision: response.revision,
    projection: runtimeRegionProjection(response),
  };
}

export function runtimeEditorPatches(
  patches: ProtocolEditorPatch[] | undefined,
): EditorPatch[] | undefined {
  return patches?.map((patch): EditorPatch => {
    switch (patch.type) {
      case 'Cells':
        return {
          type: 'Cells',
          data: {
            changes: patch.data.changes.map((change) => ({
              sheetIndex: change.sheetIndex,
              row: change.row,
              col: change.col,
              value: runtimeCellValue(change.value),
            })),
          },
        };
      case 'Layout':
        return {
          type: 'Layout',
          data: {
            patch: {
              sheetIndex: patch.data.patch.sheetIndex,
              columnWidths: copyNumericRecord(patch.data.patch.columnWidths),
              rowHeights: copyNumericRecord(patch.data.patch.rowHeights),
            },
          },
        };
      case 'SheetInserted':
        return {
          type: 'SheetInserted',
          data: {
            patch: {
              sheetIndex: patch.data.patch.sheetIndex,
              sheet: runtimeSheetManifest(patch.data.patch.sheet),
            },
          },
        };
      case 'SheetDeleted':
      case 'SheetInvalidated':
        return { type: patch.type, data: { patch: { ...patch.data.patch } } };
      case 'SheetsReplaced':
        return {
          type: 'SheetsReplaced',
          data: {
            patch: {
              startIndex: patch.data.patch.startIndex,
              sheets: patch.data.patch.sheets.map(runtimeSheetManifest),
            },
          },
        };
      case 'RowInserted':
        return { type: 'RowInserted', data: { patch: { ...patch.data.patch } } };
      case 'RowDeleted':
        return { type: 'RowDeleted', data: { patch: { ...patch.data.patch } } };
      case 'ColumnInserted':
        return { type: 'ColumnInserted', data: { patch: { ...patch.data.patch } } };
      case 'ColumnDeleted':
        return { type: 'ColumnDeleted', data: { patch: { ...patch.data.patch } } };
      case 'ResyncRequired':
        return { type: 'ResyncRequired', data: { patch: { ...patch.data.patch } } };
      default:
        return assertNever(patch);
    }
  });
}

export function runtimeSheetExtents(
  extents: ProtocolSheetExtent[] | undefined,
): SheetExtent[] | undefined {
  return extents?.map((extent) => ({ ...extent }));
}

function runtimeSheetManifest(sheet: ProtocolSheetManifest): SheetManifest {
  return {
    name: sheet.name,
    extent: { ...sheet.extent },
    layout: {
      columnWidths: copyNumericRecord(sheet.layout.columnWidths),
      rowHeights: copyNumericRecord(sheet.layout.rowHeights),
    },
  };
}

function runtimeCellValue(value: ProtocolCellValue): CellValue {
  return {
    type: 'cell',
    kind: value.kind,
    raw: value.raw,
    display: value.display,
    formula: value.formula
      ? {
          formula: value.formula.formula,
          cachedValue: runtimeCellValue(value.formula.cachedValue),
          error: value.formula.error,
        }
      : undefined,
    format: value.format ? { ...value.format } : undefined,
  };
}

function runtimeRegionMetadata(
  metadata: ProtocolSheetRegionMetadata,
): SheetRegionMetadata {
  return {
    merges: (metadata.merges ?? []).map((merge) => ({ ...merge })),
    cellFormats: Object.fromEntries(
      Object.entries(metadata.cellFormats ?? {}).map(([key, value]) => [key, { ...value }]),
    ),
    cellStyles: Object.fromEntries(
      Object.entries(metadata.cellStyles ?? {}).map(([key, value]) => [key, { ...value }]),
    ),
  };
}

function copyNumericRecord<T>(source: Record<number, T> | undefined): Record<number, T> | undefined {
  return source ? { ...source } : undefined;
}

function assertNever(value: never): never {
  throw new Error(`Unsupported editor patch: ${JSON.stringify(value)}`);
}
