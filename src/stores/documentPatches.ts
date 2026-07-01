import type {
  CellValue,
  ColumnDeletedPatch,
  ColumnInsertedPatch,
  EditorPatch,
  FileData,
  RowDeletedPatch,
  RowInsertedPatch,
  SheetCellChange,
  SheetDeletedPatch,
  SheetInsertedPatch,
} from "@/types";

export function applyDocumentPatches(
  data: FileData | null,
  patches: EditorPatch[] | undefined
): FileData | null {
  let nextData = data;
  for (const patch of patches ?? []) {
    switch (patch.type) {
      case "FullSnapshot":
        nextData = applySnapshot(nextData, patch.data.fileData);
        break;
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
      case "SheetSnapshot":
        nextData = applySheetSnapshot(nextData, patch.data.sheetIndex, patch.data.sheet);
        break;
      default:
        assertNever(patch);
    }
  }
  return nextData;
}

function applyRowInserted(data: FileData | null, patch: RowInsertedPatch): FileData | null {
  const sheet = data?.sheets[patch.sheetIndex];
  if (!data || !sheet) return data;

  const rows = [...sheet.rows];
  rows.splice(patch.rowIndex, 0, [...patch.row]);

  const nextSheet = {
    ...sheet,
    rows,
    merges: patch.merges,
    rowHeights: normalizeNumberRecord(patch.rowHeights),
    rich: patch.rich ?? sheet.rich,
  };
  if (patch.rowHeight !== undefined) {
    nextSheet.rowHeights = {
      ...(nextSheet.rowHeights ?? {}),
      [patch.rowIndex]: patch.rowHeight,
    };
  }
  return replaceSheet(data, patch.sheetIndex, nextSheet);
}

function applyRowDeleted(data: FileData | null, patch: RowDeletedPatch): FileData | null {
  const sheet = data?.sheets[patch.sheetIndex];
  if (!data || !sheet) return data;

  const rows = [...sheet.rows];
  if (patch.rowIndex < rows.length) {
    rows.splice(patch.rowIndex, 1);
  }

  return replaceSheet(data, patch.sheetIndex, {
    ...sheet,
    rows,
    merges: patch.merges,
    rowHeights: normalizeNumberRecord(patch.rowHeights),
    rich: patch.rich ?? sheet.rich,
  });
}

function applyColumnInserted(data: FileData | null, patch: ColumnInsertedPatch): FileData | null {
  const sheet = data?.sheets[patch.sheetIndex];
  if (!data || !sheet) return data;

  const rowCount = Math.max(sheet.rows.length, patch.column.length);
  const rows = Array.from({ length: rowCount }, (_, rowIndex) => {
    const row = [...(sheet.rows[rowIndex] ?? [])];
    row.splice(Math.min(patch.colIndex, row.length), 0, patch.column[rowIndex] ?? null);
    return row;
  });

  const nextSheet = {
    ...sheet,
    rows,
    merges: patch.merges,
    columnWidths: normalizeNumberRecord(patch.columnWidths),
    rich: patch.rich ?? sheet.rich,
  };
  if (patch.columnWidth !== undefined) {
    nextSheet.columnWidths = {
      ...(nextSheet.columnWidths ?? {}),
      [patch.colIndex]: patch.columnWidth,
    };
  }
  return replaceSheet(data, patch.sheetIndex, nextSheet);
}

function applyColumnDeleted(data: FileData | null, patch: ColumnDeletedPatch): FileData | null {
  const sheet = data?.sheets[patch.sheetIndex];
  if (!data || !sheet) return data;

  const rows = sheet.rows.map((row) => {
    const nextRow = [...row];
    if (patch.colIndex < nextRow.length) {
      nextRow.splice(patch.colIndex, 1);
    }
    return nextRow;
  });

  return replaceSheet(data, patch.sheetIndex, {
    ...sheet,
    rows,
    merges: patch.merges,
    columnWidths: normalizeNumberRecord(patch.columnWidths),
    rich: patch.rich ?? sheet.rich,
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

function applySnapshot(current: FileData | null, snapshot: FileData): FileData {
  return {
    ...snapshot,
    path: current?.path ?? snapshot.path,
    fileName: current?.fileName ?? snapshot.fileName,
  };
}

function applySheetSnapshot(
  data: FileData | null,
  sheetIndex: number,
  sheetSnapshot: FileData["sheets"][number]
): FileData | null {
  if (!data || !data.sheets[sheetIndex]) return data;
  const nextData = {
    ...data,
    sheets: [...data.sheets],
  };
  nextData.sheets[sheetIndex] = sheetSnapshot;
  return nextData;
}

function replaceSheet(
  data: FileData,
  sheetIndex: number,
  sheet: FileData["sheets"][number]
): FileData {
  const nextData = {
    ...data,
    sheets: [...data.sheets],
  };
  nextData.sheets[sheetIndex] = sheet;
  return nextData;
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
    rows[row].push(null);
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

function normalizeNumberRecord(
  record: Record<number, number> | undefined
): Record<number, number> | undefined {
  if (!record || !Object.keys(record).length) return undefined;
  return { ...record };
}

function assertNever(value: never): never {
  throw new Error(`Unhandled editor patch: ${JSON.stringify(value)}`);
}
