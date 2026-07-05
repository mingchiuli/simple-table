import type {
  CellValue,
  CellFormatProjection,
  CellStyleProjection,
  EditorPatch,
  FileData,
  SheetCellChange,
  SheetDeletedPatch,
  SheetInsertedPatch,
  SheetUpdatedPatch,
  SheetShapePatch,
  RowsInsertedPatch,
  RowsDeletedPatch,
  ColumnsInsertedPatch,
  ColumnsDeletedPatch,
} from "@/types";
import { blankCell } from "@/utils/cellValue";
import { defaultRichProjection } from "@/types";
import {
  cellKey,
  deleteRichColumns,
  deleteRichRows,
  shiftRichColumns,
  shiftRichRows,
} from "@/utils/cellAddress";

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
      case "RowsInserted":
        nextData = applyRowsInserted(nextData, patch.data.patch);
        break;
      case "RowsDeleted":
        nextData = applyRowsDeleted(nextData, patch.data.patch);
        break;
      case "ColumnsInserted":
        nextData = applyColumnsInserted(nextData, patch.data.patch);
        break;
      case "ColumnsDeleted":
        nextData = applyColumnsDeleted(nextData, patch.data.patch);
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

function applyRowsInserted(data: FileData | null, patch: RowsInsertedPatch): FileData | null {
  const sheet = data?.sheets[patch.sheetIndex];
  if (!data || !sheet || patch.rows.length === 0) return data;
  const rows = [...sheet.rows];
  rows.splice(
    Math.min(patch.rowIndex, rows.length),
    0,
    ...patch.rows.map((row, rowOffset) =>
      row.map((cell, colIndex) => applyPatchDisplay(cell, patch.displays?.[rowOffset]?.[colIndex]))
    )
  );
  const patchedSheet = applyInsertedRowsMetadata(
    insertSheetRows({ ...sheet, rows }, patch.rowIndex, patch.rows.length),
    patch.rowIndex,
    patch.formats ?? [],
    patch.styles ?? []
  );
  return replaceSheet(data, patch.sheetIndex, patchedSheet);
}

function applyRowsDeleted(data: FileData | null, patch: RowsDeletedPatch): FileData | null {
  const sheet = data?.sheets[patch.sheetIndex];
  if (!data || !sheet || patch.count === 0) return data;
  const rows = [...sheet.rows];
  if (patch.rowIndex < rows.length) {
    rows.splice(patch.rowIndex, patch.count);
  }
  return replaceSheet(data, patch.sheetIndex, deleteSheetRows({ ...sheet, rows }, patch.rowIndex, patch.count));
}

function applyColumnsInserted(data: FileData | null, patch: ColumnsInsertedPatch): FileData | null {
  const sheet = data?.sheets[patch.sheetIndex];
  if (!data || !sheet) return data;
  const rows = [...sheet.rows];
  const targetRows = Math.max(rows.length, patch.values.length);
  for (let rowIndex = 0; rowIndex < targetRows; rowIndex += 1) {
    const row = [...(rows[rowIndex] ?? [])];
    while (row.length < patch.colIndex) {
      row.push(blankCell());
    }
    row.splice(
      Math.min(patch.colIndex, row.length),
      0,
      applyPatchDisplay(patch.values[rowIndex] ?? blankCell(), patch.displays?.[rowIndex])
    );
    rows[rowIndex] = row;
  }
  const patchedSheet = applyInsertedColumnsMetadata(
    insertSheetColumns({ ...sheet, rows }, patch.colIndex, 1),
    patch.colIndex,
    patch.formats ?? [],
    patch.styles ?? []
  );
  return replaceSheet(data, patch.sheetIndex, patchedSheet);
}

function applyColumnsDeleted(data: FileData | null, patch: ColumnsDeletedPatch): FileData | null {
  const sheet = data?.sheets[patch.sheetIndex];
  if (!data || !sheet || patch.count === 0) return data;
  const rows = sheet.rows.map((row) => {
    const nextRow = [...row];
    if (patch.colIndex < nextRow.length) {
      nextRow.splice(patch.colIndex, patch.count);
    }
    return nextRow;
  });
  return replaceSheet(data, patch.sheetIndex, deleteSheetColumns({ ...sheet, rows }, patch.colIndex, patch.count));
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

function applyInsertedRowsMetadata(
  sheet: FileData["sheets"][number],
  rowIndex: number,
  formats: (CellFormatProjection | null | undefined)[][],
  styles: (CellStyleProjection | null | undefined)[][]
): FileData["sheets"][number] {
  let nextSheet = sheet;
  for (const [rowOffset, rowFormats] of formats.entries()) {
    for (const [colIndex, format] of rowFormats.entries()) {
      nextSheet = patchCellMetadata(
        nextSheet,
        rowIndex + rowOffset,
        colIndex,
        format,
        styles[rowOffset]?.[colIndex]
      );
    }
  }
  return nextSheet;
}

function applyInsertedColumnsMetadata(
  sheet: FileData["sheets"][number],
  colIndex: number,
  formats: (CellFormatProjection | null | undefined)[],
  styles: (CellStyleProjection | null | undefined)[]
): FileData["sheets"][number] {
  let nextSheet = sheet;
  for (const [rowIndex, format] of formats.entries()) {
    nextSheet = patchCellMetadata(nextSheet, rowIndex, colIndex, format, styles[rowIndex]);
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

function insertSheetRows(
  sheet: FileData["sheets"][number],
  rowIndex: number,
  count: number
): FileData["sheets"][number] {
  return {
    ...sheet,
    merges: sheet.merges.map((merge) => insertRowsIntoMerge(merge, rowIndex, count)),
    rowHeights: shiftNumberRecordOnInsert(sheet.rowHeights, rowIndex, count),
    rich: shiftRichRows(sheet.rich, rowIndex, count),
  };
}

function deleteSheetRows(
  sheet: FileData["sheets"][number],
  rowIndex: number,
  count: number
): FileData["sheets"][number] {
  return {
    ...sheet,
    merges: sheet.merges.flatMap((merge) => {
      const shifted = deleteRowsFromMerge(merge, rowIndex, count);
      return shifted ? [shifted] : [];
    }),
    rowHeights: shiftNumberRecordOnDelete(sheet.rowHeights, rowIndex, count),
    rich: deleteRichRows(sheet.rich, rowIndex, count),
  };
}

function insertSheetColumns(
  sheet: FileData["sheets"][number],
  colIndex: number,
  count: number
): FileData["sheets"][number] {
  return {
    ...sheet,
    merges: sheet.merges.map((merge) => insertColumnsIntoMerge(merge, colIndex, count)),
    columnWidths: shiftNumberRecordOnInsert(sheet.columnWidths, colIndex, count),
    rich: shiftRichColumns(sheet.rich, colIndex, count),
  };
}

function deleteSheetColumns(
  sheet: FileData["sheets"][number],
  colIndex: number,
  count: number
): FileData["sheets"][number] {
  return {
    ...sheet,
    merges: sheet.merges.flatMap((merge) => {
      const shifted = deleteColumnsFromMerge(merge, colIndex, count);
      return shifted ? [shifted] : [];
    }),
    columnWidths: shiftNumberRecordOnDelete(sheet.columnWidths, colIndex, count),
    rich: deleteRichColumns(sheet.rich, colIndex, count),
  };
}

function insertRowsIntoMerge(
  merge: FileData["sheets"][number]["merges"][number],
  rowIndex: number,
  count: number
) {
  if (merge.startRow >= rowIndex) {
    return { ...merge, startRow: merge.startRow + count, endRow: merge.endRow + count };
  }
  if (merge.endRow >= rowIndex) {
    return { ...merge, endRow: merge.endRow + count };
  }
  return merge;
}

function insertColumnsIntoMerge(
  merge: FileData["sheets"][number]["merges"][number],
  colIndex: number,
  count: number
) {
  if (merge.startCol >= colIndex) {
    return { ...merge, startCol: merge.startCol + count, endCol: merge.endCol + count };
  }
  if (merge.endCol >= colIndex) {
    return { ...merge, endCol: merge.endCol + count };
  }
  return merge;
}

function deleteRowsFromMerge(
  merge: FileData["sheets"][number]["merges"][number],
  rowIndex: number,
  count: number
) {
  const rows = deleteIndexRange(merge.startRow, merge.endRow, rowIndex, count);
  if (!rows) return null;
  return { ...merge, startRow: rows.start, endRow: rows.end };
}

function deleteColumnsFromMerge(
  merge: FileData["sheets"][number]["merges"][number],
  colIndex: number,
  count: number
) {
  const columns = deleteIndexRange(merge.startCol, merge.endCol, colIndex, count);
  if (!columns) return null;
  return { ...merge, startCol: columns.start, endCol: columns.end };
}

function deleteIndexRange(
  start: number,
  end: number,
  deletedStart: number,
  count: number
): { start: number; end: number } | null {
  const deletedEnd = deletedStart + count - 1;
  if (end < deletedStart) return { start, end };
  if (start > deletedEnd) return { start: start - count, end: end - count };
  if (start < deletedStart && end > deletedEnd) return { start, end: end - count };
  if (start < deletedStart) return { start, end: deletedStart - 1 };
  if (end > deletedEnd) return { start: deletedStart, end: end - count };
  return null;
}

function shiftNumberRecordOnInsert(
  current: Record<number, number> | undefined,
  index: number,
  count: number
): Record<number, number> | undefined {
  const next: Record<number, number> = {};
  for (const [rawKey, value] of Object.entries(current ?? {})) {
    const key = Number(rawKey);
    next[key >= index ? key + count : key] = value;
  }
  return Object.keys(next).length ? next : undefined;
}

function shiftNumberRecordOnDelete(
  current: Record<number, number> | undefined,
  index: number,
  count: number
): Record<number, number> | undefined {
  const next: Record<number, number> = {};
  for (const [rawKey, value] of Object.entries(current ?? {})) {
    const key = Number(rawKey);
    if (key < index) {
      next[key] = value;
    } else if (key >= index + count) {
      next[key - count] = value;
    }
  }
  return Object.keys(next).length ? next : undefined;
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
