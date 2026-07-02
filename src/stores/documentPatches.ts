import type {
  CellValue,
  ColumnDeletedPatch,
  ColumnInsertedPatch,
  DrawingProjection,
  EditorPatch,
  FileData,
  MergeRange,
  RowDeletedPatch,
  RowInsertedPatch,
  SheetCellChange,
  SheetDeletedPatch,
  SheetInsertedPatch,
  SheetShapePatch,
  SheetRichProjection,
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
      case "SheetInserted":
        nextData = applySheetInserted(nextData, patch.data.patch);
        break;
      case "SheetDeleted":
        nextData = applySheetDeleted(nextData, patch.data.patch);
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

function applyRowInserted(data: FileData | null, patch: RowInsertedPatch): FileData | null {
  const sheet = data?.sheets[patch.sheetIndex];
  if (!data || !sheet) return data;
  const rows = [...sheet.rows];
  rows.splice(Math.min(patch.rowIndex, rows.length), 0, patch.row);
  return replaceSheet(data, patch.sheetIndex, {
    ...sheet,
    rows,
    merges: shiftRowMergesOnInsert(sheet.merges, patch.rowIndex),
    rowHeights: setOptionalLayoutValue(
      shiftLayoutMapOnInsert(sheet.rowHeights, patch.rowIndex),
      patch.rowIndex,
      patch.rowHeight
    ),
    rich: shiftRichProjection(sheet.rich, { axis: "row", kind: "insert", index: patch.rowIndex }),
  });
}

function applyRowDeleted(data: FileData | null, patch: RowDeletedPatch): FileData | null {
  const sheet = data?.sheets[patch.sheetIndex];
  if (!data || !sheet) return data;
  const rows = [...sheet.rows];
  if (patch.rowIndex < rows.length) rows.splice(patch.rowIndex, 1);
  return replaceSheet(data, patch.sheetIndex, {
    ...sheet,
    rows,
    merges: shiftRowMergesOnDelete(sheet.merges, patch.rowIndex),
    rowHeights: shiftLayoutMapOnDelete(sheet.rowHeights, patch.rowIndex),
    rich: shiftRichProjection(sheet.rich, { axis: "row", kind: "delete", index: patch.rowIndex }),
  });
}

function applyColumnInserted(data: FileData | null, patch: ColumnInsertedPatch): FileData | null {
  const sheet = data?.sheets[patch.sheetIndex];
  if (!data || !sheet) return data;
  const rows = sheet.rows.map((row, rowIndex) => {
    const nextRow = [...row];
    nextRow.splice(Math.min(patch.columnIndex, nextRow.length), 0, patch.values[rowIndex] ?? blankCell());
    return nextRow;
  });
  return replaceSheet(data, patch.sheetIndex, {
    ...sheet,
    rows,
    merges: shiftColumnMergesOnInsert(sheet.merges, patch.columnIndex),
    columnWidths: setOptionalLayoutValue(
      shiftLayoutMapOnInsert(sheet.columnWidths, patch.columnIndex),
      patch.columnIndex,
      patch.columnWidth
    ),
    rich: shiftRichProjection(sheet.rich, { axis: "column", kind: "insert", index: patch.columnIndex }),
  });
}

function applyColumnDeleted(data: FileData | null, patch: ColumnDeletedPatch): FileData | null {
  const sheet = data?.sheets[patch.sheetIndex];
  if (!data || !sheet) return data;
  const rows = sheet.rows.map((row) => {
    const nextRow = [...row];
    if (patch.columnIndex < nextRow.length) nextRow.splice(patch.columnIndex, 1);
    return nextRow;
  });
  return replaceSheet(data, patch.sheetIndex, {
    ...sheet,
    rows,
    merges: shiftColumnMergesOnDelete(sheet.merges, patch.columnIndex),
    columnWidths: shiftLayoutMapOnDelete(sheet.columnWidths, patch.columnIndex),
    rich: shiftRichProjection(sheet.rich, { axis: "column", kind: "delete", index: patch.columnIndex }),
  });
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

function shiftLayoutMapOnInsert(
  current: Record<number, number> | undefined,
  index: number
): Record<number, number> | undefined {
  if (!current) return undefined;
  const next: Record<number, number> = {};
  for (const [key, value] of Object.entries(current)) {
    const numericKey = Number(key);
    next[numericKey >= index ? numericKey + 1 : numericKey] = value;
  }
  return Object.keys(next).length ? next : undefined;
}

function shiftLayoutMapOnDelete(
  current: Record<number, number> | undefined,
  index: number
): Record<number, number> | undefined {
  if (!current) return undefined;
  const next: Record<number, number> = {};
  for (const [key, value] of Object.entries(current)) {
    const numericKey = Number(key);
    if (numericKey === index) continue;
    next[numericKey > index ? numericKey - 1 : numericKey] = value;
  }
  return Object.keys(next).length ? next : undefined;
}

function setOptionalLayoutValue(
  current: Record<number, number> | undefined,
  index: number,
  value: number | undefined
): Record<number, number> | undefined {
  if (value === undefined) return current;
  return {
    ...(current ?? {}),
    [index]: value,
  };
}

function shiftRowMergesOnInsert(merges: MergeRange[], rowIndex: number): MergeRange[] {
  return merges.map((merge) => {
    if (merge.startRow >= rowIndex) {
      return { ...merge, startRow: merge.startRow + 1, endRow: merge.endRow + 1 };
    }
    if (merge.endRow >= rowIndex) {
      return { ...merge, endRow: merge.endRow + 1 };
    }
    return { ...merge };
  });
}

function shiftRowMergesOnDelete(merges: MergeRange[], rowIndex: number): MergeRange[] {
  return merges.flatMap((merge) => {
    if (merge.startRow === rowIndex && merge.endRow === rowIndex) return [];
    let next = { ...merge };
    if (next.startRow > rowIndex) {
      next.startRow -= 1;
      next.endRow -= 1;
    } else if (next.endRow >= rowIndex) {
      next.endRow = Math.max(0, next.endRow - 1);
    }
    return next.startRow <= next.endRow && next.startCol <= next.endCol ? [next] : [];
  });
}

function shiftColumnMergesOnInsert(merges: MergeRange[], columnIndex: number): MergeRange[] {
  return merges.map((merge) => {
    if (merge.startCol >= columnIndex) {
      return { ...merge, startCol: merge.startCol + 1, endCol: merge.endCol + 1 };
    }
    if (merge.endCol >= columnIndex) {
      return { ...merge, endCol: merge.endCol + 1 };
    }
    return { ...merge };
  });
}

function shiftColumnMergesOnDelete(merges: MergeRange[], columnIndex: number): MergeRange[] {
  return merges.flatMap((merge) => {
    if (merge.startCol === columnIndex && merge.endCol === columnIndex) return [];
    let next = { ...merge };
    if (next.startCol > columnIndex) {
      next.startCol -= 1;
      next.endCol -= 1;
    } else if (next.endCol >= columnIndex) {
      next.endCol = Math.max(0, next.endCol - 1);
    }
    return next.startRow <= next.endRow && next.startCol <= next.endCol ? [next] : [];
  });
}

type StructureShift =
  | { axis: "row"; kind: "insert" | "delete"; index: number }
  | { axis: "column"; kind: "insert" | "delete"; index: number };

function shiftRichProjection(
  rich: SheetRichProjection | undefined,
  shift: StructureShift
): SheetRichProjection | undefined {
  if (!rich) return rich;
  return {
    ...rich,
    cellFormats: shiftCellRecord(rich.cellFormats, shift),
    cellStyles: shiftCellRecord(rich.cellStyles, shift),
    drawings: rich.drawings?.flatMap((drawing) => shiftDrawing(drawing, shift)),
  };
}

function shiftCellRecord<T>(
  current: Record<string, T> | undefined,
  shift: StructureShift
): Record<string, T> | undefined {
  if (!current) return undefined;
  const next: Record<string, T> = {};
  for (const [key, value] of Object.entries(current)) {
    const position = parseExcelCellKey(key);
    if (!position) {
      next[key] = value;
      continue;
    }
    const shifted = shiftPosition(position.row, position.col, shift);
    if (!shifted) continue;
    next[toExcelCellKey(shifted.row, shifted.col)] = value;
  }
  return Object.keys(next).length ? next : undefined;
}

function shiftDrawing(
  drawing: DrawingProjection,
  shift: StructureShift
): DrawingProjection[] {
  const from = shiftPosition(drawing.fromRow, drawing.fromCol, shift);
  const to = drawing.toRow === undefined || drawing.toCol === undefined
    ? undefined
    : shiftPosition(drawing.toRow, drawing.toCol, shift);
  if (!from && !to) return [];
  return [{
    ...drawing,
    fromRow: from?.row ?? to!.row,
    fromCol: from?.col ?? to!.col,
    toRow: to?.row,
    toCol: to?.col,
  }];
}

function shiftPosition(
  row: number,
  col: number,
  shift: StructureShift
): { row: number; col: number } | null {
  if (shift.axis === "row") {
    const shiftedRow = shiftIndex(row, shift.index, shift.kind);
    return shiftedRow === null ? null : { row: shiftedRow, col };
  }
  const shiftedCol = shiftIndex(col, shift.index, shift.kind);
  return shiftedCol === null ? null : { row, col: shiftedCol };
}

function shiftIndex(
  value: number,
  index: number,
  kind: "insert" | "delete"
): number | null {
  if (kind === "insert") {
    return value >= index ? value + 1 : value;
  }
  if (value === index) return null;
  return value > index ? value - 1 : value;
}

function parseExcelCellKey(key: string): { row: number; col: number } | null {
  const match = /^([A-Z]+)([1-9][0-9]*)$/i.exec(key);
  if (!match) return null;
  let col = 0;
  for (const char of match[1].toUpperCase()) {
    col = col * 26 + char.charCodeAt(0) - 64;
  }
  return {
    row: Number(match[2]) - 1,
    col: col - 1,
  };
}

function toExcelCellKey(row: number, col: number): string {
  let index = col + 1;
  let letters = "";
  while (index > 0) {
    const rem = (index - 1) % 26;
    letters = String.fromCharCode(65 + rem) + letters;
    index = Math.floor((index - 1) / 26);
  }
  return `${letters}${row + 1}`;
}

function assertNever(value: never): never {
  throw new Error(`Unhandled editor patch: ${JSON.stringify(value)}`);
}
