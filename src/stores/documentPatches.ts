import type {
  CellValue,
  CellFormatProjection,
  CellStyleProjection,
  EditorPatch,
  FileData,
  ColumnDeletedPatch,
  ColumnInsertedPatch,
  RowDeletedPatch,
  RowInsertedPatch,
  SheetCellChange,
  SheetDeletedPatch,
  SheetInsertedPatch,
  SheetStructureMetadataPatch,
  SheetUpdatedPatch,
  SheetShapePatch,
} from "@/types";
import { blankCell } from "@/utils/cellValue";
import { defaultRichProjection } from "@/types";
import { cellKey } from "@/utils/cellAddress";

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
    rich: {
      ...defaultRichProjection(),
      ...(metadata.rich ?? {}),
    },
  };
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
