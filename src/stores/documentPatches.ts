import type {
  CellValue,
  CellFormatProjection,
  CellStyleProjection,
  EditorPatch,
  FileData,
  ColumnDeletedPatch,
  ColumnInsertedPatch,
  DrawingProjection,
  RowDeletedPatch,
  RowInsertedPatch,
  ReadOnlyRichProjection,
  RichProjectionPatch,
  SheetCellChange,
  SheetDeletedPatch,
  SheetInsertedPatch,
  SheetsReplacedPatch,
  SheetStructureMetadataPatch,
  SheetUpdatedPatch,
  SheetShapePatch,
} from "@/types";
import { blankCell } from "@/utils/cellValue";
import { defaultRichProjection } from "@/types";
import { cellKey, parseCellKey } from "@/utils/cellAddress";

export type PatchApplyResult = {
  data: FileData | null;
  resyncRequired: boolean;
};

export function applyDocumentPatches(
  data: FileData | null,
  patches: EditorPatch[] | undefined
): PatchApplyResult {
  let nextData = data;
  let resyncRequired = false;
  for (const patch of patches ?? []) {
    switch (patch.type) {
      case "Cells":
        nextData = applyCellChanges(nextData, patch.data.changes);
        break;
      case "Layout":
        nextData = applyLayoutPatch(
          nextData,
          patch.data.patch.sheetIndex,
          patch.data.patch.columnWidths ?? {},
          patch.data.patch.rowHeights ?? {}
        );
        break;
      case "SheetInserted":
        nextData = applySheetInserted(nextData, patch.data.patch);
        break;
      case "SheetDeleted":
        nextData = applySheetDeleted(nextData, patch.data.patch);
        break;
      case "SheetUpdated":
        nextData = applySheetUpdated(nextData, patch.data.patch);
        break;
      case "SheetsReplaced":
        nextData = applySheetsReplaced(nextData, patch.data.patch);
        break;
      case "RowInserted":
        nextData = applyRowInserted(nextData, patch.data.patch);
        break;
      case "RowDeleted":
        nextData = applyRowDeleted(nextData, patch.data.patch);
        break;
      case "ColumnInserted":
        nextData = applyColumnInserted(nextData, patch.data.patch);
        break;
      case "ColumnDeleted":
        nextData = applyColumnDeleted(nextData, patch.data.patch);
        break;
      case "SheetShape":
        nextData = applySheetShape(nextData, patch.data.patch);
        break;
      case "ResyncRequired":
        resyncRequired = true;
        break;
      default:
        assertNever(patch);
    }
  }
  return { data: nextData, resyncRequired };
}

function applySheetInserted(data: FileData | null, patch: SheetInsertedPatch): FileData | null {
  if (!data) return data;
  const sheets = [...data.sheets];
  sheets.splice(Math.min(patch.sheetIndex, sheets.length), 0, patch.sheet);
  return { ...data, sheets };
}

function applySheetDeleted(data: FileData | null, patch: SheetDeletedPatch): FileData | null {
  if (!data) return data;
  const sheets = [...data.sheets];
  if (patch.sheetIndex < sheets.length) {
    sheets.splice(patch.sheetIndex, 1);
  }
  return { ...data, sheets };
}

function applySheetUpdated(data: FileData | null, patch: SheetUpdatedPatch): FileData | null {
  if (!data) return data;
  return replaceSheet(data, patch.sheetIndex, patch.sheet);
}

function applySheetsReplaced(data: FileData | null, patch: SheetsReplacedPatch): FileData | null {
  if (!data) return data;
  const sheets = data.sheets.slice(0, Math.max(0, Math.min(patch.startIndex, data.sheets.length)));
  sheets.push(...patch.sheets);
  return { ...data, sheets };
}

function applyRowInserted(data: FileData | null, patch: RowInsertedPatch): FileData | null {
  const sheet = data?.sheets[patch.sheetIndex];
  if (!data || !sheet) return data;
  const rows = [...sheet.rows];
  while (rows.length < patch.rowIndex) {
    rows.push([]);
  }
  rows.splice(patch.rowIndex, 0, ...patch.rows.map((row) => [...row]));
  return replaceSheet(data, patch.sheetIndex, applyStructureMetadata({ ...sheet, rows }, patch.metadata));
}

function applyRowDeleted(data: FileData | null, patch: RowDeletedPatch): FileData | null {
  const sheet = data?.sheets[patch.sheetIndex];
  if (!data || !sheet) return data;
  const rows = [...sheet.rows];
  if (patch.rowIndex < rows.length) {
    rows.splice(patch.rowIndex, patch.count);
  }
  return replaceSheet(data, patch.sheetIndex, applyStructureMetadata({ ...sheet, rows }, patch.metadata));
}

function applyColumnInserted(data: FileData | null, patch: ColumnInsertedPatch): FileData | null {
  const sheet = data?.sheets[patch.sheetIndex];
  if (!data || !sheet) return data;
  const rowCount = Math.max(sheet.rows.length, patch.values.length);
  const rows = Array.from({ length: rowCount }, (_, rowIndex) => {
    const row = [...(sheet.rows[rowIndex] ?? [])];
    const insertAt = Math.min(patch.colIndex, row.length);
    row.splice(insertAt, 0, patch.values[rowIndex] ?? blankCell());
    return row;
  });
  return replaceSheet(data, patch.sheetIndex, applyStructureMetadata({ ...sheet, rows }, patch.metadata));
}

function applyColumnDeleted(data: FileData | null, patch: ColumnDeletedPatch): FileData | null {
  const sheet = data?.sheets[patch.sheetIndex];
  if (!data || !sheet) return data;
  const rows = sheet.rows.map((row) => {
    const nextRow = [...row];
    if (patch.colIndex < nextRow.length) {
      nextRow.splice(patch.colIndex, patch.count);
    }
    return nextRow;
  });
  return replaceSheet(data, patch.sheetIndex, applyStructureMetadata({ ...sheet, rows }, patch.metadata));
}

function applyStructureMetadata(
  sheet: FileData["sheets"][number],
  metadata: SheetStructureMetadataPatch
): FileData["sheets"][number] {
  return {
    ...sheet,
    merges: metadata.merges,
    columnWidths: metadata.columnWidths,
    rowHeights: metadata.rowHeights,
    rich: applyRichProjectionPatch(sheet.rich, metadata.rich),
  };
}

function applyRichProjectionPatch(
  current: ReadOnlyRichProjection | undefined,
  patch: RichProjectionPatch
): ReadOnlyRichProjection {
  const base = normalizeRichProjection(current);
  const projection = normalizeRichProjection(patch.projection);

  if (patch.scope.type === "all") {
    return projection;
  }

  if (patch.scope.type === "rows") {
    const start = patch.scope.start;
    return {
      ...base,
      ...projection,
      cellFormats: mergeCellMetadataByScope(base.cellFormats, projection.cellFormats, (row) => row >= start),
      cellStyles: mergeCellMetadataByScope(base.cellStyles, projection.cellStyles, (row) => row >= start),
      hyperlinks: mergeCellMetadataByScope(base.hyperlinks, projection.hyperlinks, (row) => row >= start),
      hiddenRows: mergeNumberArrayByScope(base.hiddenRows, projection.hiddenRows, (row) => row >= start),
      hiddenColumns: base.hiddenColumns,
      freezePane: mergeFreezePaneByScope(base.freezePane, projection.freezePane, (row) => row >= start),
      drawings: mergeDrawingsByScope(base.drawings, projection.drawings, (drawing) => drawingRowScopeAffected(drawing, start)),
    };
  }

  const start = patch.scope.start;
  return {
    ...base,
    ...projection,
    cellFormats: mergeCellMetadataByScope(base.cellFormats, projection.cellFormats, (_row, col) => col >= start),
    cellStyles: mergeCellMetadataByScope(base.cellStyles, projection.cellStyles, (_row, col) => col >= start),
    hyperlinks: mergeCellMetadataByScope(base.hyperlinks, projection.hyperlinks, (_row, col) => col >= start),
    hiddenRows: base.hiddenRows,
    hiddenColumns: mergeNumberArrayByScope(base.hiddenColumns, projection.hiddenColumns, (col) => col >= start),
    freezePane: mergeFreezePaneByScope(base.freezePane, projection.freezePane, (_row, col) => col >= start),
    drawings: mergeDrawingsByScope(base.drawings, projection.drawings, (drawing) => drawingColumnScopeAffected(drawing, start)),
  };
}

function normalizeRichProjection(
  projection: ReadOnlyRichProjection | undefined
): ReadOnlyRichProjection {
  return {
    ...defaultRichProjection(),
    ...(projection ?? {}),
    cellFormats: { ...(projection?.cellFormats ?? {}) },
    cellStyles: { ...(projection?.cellStyles ?? {}) },
    hyperlinks: { ...(projection?.hyperlinks ?? {}) },
    drawings: [...(projection?.drawings ?? [])],
    hiddenRows: [...(projection?.hiddenRows ?? [])],
    hiddenColumns: [...(projection?.hiddenColumns ?? [])],
  };
}

function mergeCellMetadataByScope<T>(
  current: Record<string, T> | undefined,
  patch: Record<string, T> | undefined,
  isAffected: (row: number, col: number) => boolean
): Record<string, T> {
  const next: Record<string, T> = {};
  for (const [key, value] of Object.entries(current ?? {})) {
    const address = parseCellKey(key);
    if (!address || !isAffected(address.row, address.col)) {
      next[key] = value;
    }
  }
  return { ...next, ...(patch ?? {}) };
}

function mergeNumberArrayByScope(
  current: number[] | undefined,
  patch: number[] | undefined,
  isAffected: (value: number) => boolean
): number[] {
  const retained = (current ?? []).filter((value) => !isAffected(value));
  return Array.from(new Set([...retained, ...(patch ?? [])])).sort((a, b) => a - b);
}

function mergeFreezePaneByScope(
  current: ReadOnlyRichProjection["freezePane"],
  patch: ReadOnlyRichProjection["freezePane"],
  isAffected: (row: number, col: number) => boolean
): ReadOnlyRichProjection["freezePane"] {
  const address = current?.topLeftCell ? parseCellKey(current.topLeftCell) : null;
  return address && !isAffected(address.row, address.col) ? current : patch;
}

function mergeDrawingsByScope(
  current: ReadOnlyRichProjection["drawings"],
  patch: ReadOnlyRichProjection["drawings"],
  isAffected: (drawing: DrawingProjection) => boolean
): ReadOnlyRichProjection["drawings"] {
  return [...(current ?? []).filter((drawing) => !isAffected(drawing)), ...(patch ?? [])];
}

function drawingRowScopeAffected(
  drawing: DrawingProjection,
  rowIndex: number
): boolean {
  return drawing.fromRow >= rowIndex || (drawing.toRow != null && drawing.toRow >= rowIndex);
}

function drawingColumnScopeAffected(
  drawing: DrawingProjection,
  colIndex: number
): boolean {
  return drawing.fromCol >= colIndex || (drawing.toCol != null && drawing.toCol >= colIndex);
}

function applySheetShape(data: FileData | null, patch: SheetShapePatch): FileData | null {
  const sheet = data?.sheets[patch.sheetIndex];
  if (!data || !sheet) return data;
  const rows = [...sheet.rows];
  rows.length = patch.rowLengths.length;
  for (let rowIndex = 0; rowIndex < patch.rowLengths.length; rowIndex += 1) {
    const targetLength = patch.rowLengths[rowIndex] ?? 0;
    const row = [...(rows[rowIndex] ?? [])];
    row.length = targetLength;
    while (row.length < targetLength) {
      row.push(blankCell());
    }
    rows[rowIndex] = row;
  }
  return replaceSheet(data, patch.sheetIndex, {
    ...sheet,
    rows,
  });
}

function replaceSheet(
  data: FileData,
  sheetIndex: number,
  sheet: FileData["sheets"][number]
): FileData {
  const sheets = [...data.sheets];
  sheets[sheetIndex] = sheet;
  return { ...data, sheets };
}

function applyCellChanges(data: FileData | null, changes: SheetCellChange[]): FileData | null {
  if (!data) return null;
  if (!changes.length) return data;

  const nextData: FileData = {
    ...data,
    sheets: [...data.sheets],
  };
  const changesBySheet = new Map<number, SheetCellChange[]>();
  for (const change of changes) {
    const existing = changesBySheet.get(change.sheetIndex) ?? [];
    existing.push(change);
    changesBySheet.set(change.sheetIndex, existing);
  }

  for (const [sheetIndex, sheetChanges] of changesBySheet) {
    const sheet = data.sheets[sheetIndex];
    if (!sheet) continue;
    const rows = [...sheet.rows];
    nextData.sheets[sheetIndex] = { ...sheet, rows };
    for (const change of sheetChanges) {
      ensureCellExists(rows, change.row, change.col);
      rows[change.row][change.col] = applyPatchDisplay(change.value, change.display);
    }
    nextData.sheets[sheetIndex] = applyCellChangesMetadata(
      nextData.sheets[sheetIndex],
      sheetChanges
    );
  }

  return nextData;
}

function applyPatchDisplay(cell: CellValue, display: string | undefined): CellValue {
  if (display === undefined || cell.display === display) {
    return cell;
  }
  return { ...cell, display };
}

function applyCellChangesMetadata(
  sheet: FileData["sheets"][number],
  changes: SheetCellChange[]
): FileData["sheets"][number] {
  let nextSheet = sheet;
  for (const change of changes) {
    nextSheet = patchCellMetadata(nextSheet, change.row, change.col, change.format, change.style);
  }
  return nextSheet;
}

function patchCellMetadata(
  sheet: FileData["sheets"][number],
  row: number,
  col: number,
  format: CellFormatProjection | null | undefined,
  style: CellStyleProjection | null | undefined
): FileData["sheets"][number] {
  if (!format && !style) return sheet;

  const rich = {
    ...defaultRichProjection(),
    ...(sheet.rich ?? {}),
    cellFormats: { ...(sheet.rich?.cellFormats ?? {}) },
    cellStyles: { ...(sheet.rich?.cellStyles ?? {}) },
  };
  const key = cellKey(row, col);
  if (format) {
    rich.cellFormats[key] = format;
  }
  if (style) {
    rich.cellStyles[key] = style;
  }
  return { ...sheet, rich };
}

function applyLayoutPatch(
  data: FileData | null,
  sheetIndex: number,
  columnWidths: Record<number, number | null>,
  rowHeights: Record<number, number | null>
): FileData | null {
  const sheet = data?.sheets[sheetIndex];
  if (!data || !sheet) return data;

  const nextData = {
    ...data,
    sheets: [...data.sheets],
  };
  nextData.sheets[sheetIndex] = {
    ...sheet,
    columnWidths: patchNumberRecord(sheet.columnWidths, columnWidths),
    rowHeights: patchNumberRecord(sheet.rowHeights, rowHeights),
  };
  return nextData;
}

function ensureCellExists(rows: CellValue[][], row: number, col: number) {
  while (rows.length <= row) {
    rows.push([]);
  }
  rows[row] = [...rows[row]];
  while (rows[row].length <= col) {
    rows[row].push(blankCell());
  }
}

function patchNumberRecord(
  current: Record<number, number> | undefined,
  patch: Record<number, number | null>
): Record<number, number> | undefined {
  const next = { ...(current ?? {}) };
  for (const [key, value] of Object.entries(patch)) {
    if (value === null || value === undefined) {
      delete next[Number(key)];
    } else {
      next[Number(key)] = value;
    }
  }
  return Object.keys(next).length ? next : undefined;
}

function assertNever(value: never): never {
  throw new Error(`Unhandled editor patch: ${JSON.stringify(value)}`);
}
