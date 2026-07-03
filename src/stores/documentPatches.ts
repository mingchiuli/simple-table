import type {
  CellValue,
  EditorPatch,
  FileData,
  SheetCellChange,
  SheetMetadataPatch,
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
      case "SheetMetadata":
        nextData = applySheetMetadata(nextData, patch.data.patch);
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

function applySheetMetadata(data: FileData | null, patch: SheetMetadataPatch): FileData | null {
  const sheet = data?.sheets[patch.sheetIndex];
  if (!data || !sheet) return data;
  return replaceSheet(data, patch.sheetIndex, {
    ...sheet,
    merges: patch.merges,
    columnWidths: emptyRecordToUndefined(patch.columnWidths),
    rowHeights: emptyRecordToUndefined(patch.rowHeights),
    rich: patch.rich,
  });
}

function applyRowsInserted(data: FileData | null, patch: RowsInsertedPatch): FileData | null {
  const sheet = data?.sheets[patch.sheetIndex];
  if (!data || !sheet || patch.rows.length === 0) return data;
  const rows = [...sheet.rows];
  rows.splice(Math.min(patch.rowIndex, rows.length), 0, ...patch.rows.map((row) => [...row]));
  return replaceSheet(data, patch.sheetIndex, { ...sheet, rows });
}

function applyRowsDeleted(data: FileData | null, patch: RowsDeletedPatch): FileData | null {
  const sheet = data?.sheets[patch.sheetIndex];
  if (!data || !sheet || patch.count === 0) return data;
  const rows = [...sheet.rows];
  if (patch.rowIndex < rows.length) {
    rows.splice(patch.rowIndex, patch.count);
  }
  return replaceSheet(data, patch.sheetIndex, { ...sheet, rows });
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
      patch.values[rowIndex] ?? blankCell()
    );
    rows[rowIndex] = row;
  }
  return replaceSheet(data, patch.sheetIndex, { ...sheet, rows });
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
  return replaceSheet(data, patch.sheetIndex, { ...sheet, rows });
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
      rows[change.row][change.col] = change.value;
    }
  }

  return nextData;
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

function emptyRecordToUndefined(
  value: Record<number, number> | undefined
): Record<number, number> | undefined {
  return value && Object.keys(value).length ? value : undefined;
}

function assertNever(value: never): never {
  throw new Error(`Unhandled editor patch: ${JSON.stringify(value)}`);
}
