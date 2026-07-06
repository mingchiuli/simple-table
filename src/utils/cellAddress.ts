import type { DrawingProjection, ReadOnlyRichProjection } from "@/types";
import { defaultRichProjection } from "@/types";

export type CellAddress = {
  row: number;
  col: number;
};

export function cellKey(row: number, col: number): string {
  return `${columnName(col)}${row + 1}`;
}

export function parseCellKey(key: string): CellAddress | null {
  const match = /^([A-Z]+)([1-9]\d*)$/i.exec(key.trim());
  if (!match) return null;

  const colName = match[1].toUpperCase();
  let col = 0;
  for (const char of colName) {
    col = col * 26 + char.charCodeAt(0) - 64;
  }

  return {
    row: Number(match[2]) - 1,
    col: col - 1,
  };
}

export function shiftRichRows(
  rich: ReadOnlyRichProjection | undefined,
  rowIndex: number,
  count: number
): ReadOnlyRichProjection {
  return shiftRichProjection(rich, (address) => ({
    row: address.row >= rowIndex ? address.row + count : address.row,
    col: address.col,
  }), (drawing) => shiftDrawingRows(drawing, rowIndex, count), (row) => (
    row >= rowIndex ? row + count : row
  ), keepIndex);
}

export function shiftRichColumns(
  rich: ReadOnlyRichProjection | undefined,
  colIndex: number,
  count: number
): ReadOnlyRichProjection {
  return shiftRichProjection(rich, (address) => ({
    row: address.row,
    col: address.col >= colIndex ? address.col + count : address.col,
  }), (drawing) => shiftDrawingColumns(drawing, colIndex, count), keepIndex, (col) => (
    col >= colIndex ? col + count : col
  ));
}

export function deleteRichRows(
  rich: ReadOnlyRichProjection | undefined,
  rowIndex: number,
  count: number
): ReadOnlyRichProjection {
  return shiftRichProjection(rich, (address) => {
    const row = deleteIndex(address.row, rowIndex, count);
    return row === null ? null : { row, col: address.col };
  }, (drawing) => deleteDrawingRows(drawing, rowIndex, count), (row) => (
    deleteIndex(row, rowIndex, count)
  ), keepIndex);
}

export function deleteRichColumns(
  rich: ReadOnlyRichProjection | undefined,
  colIndex: number,
  count: number
): ReadOnlyRichProjection {
  return shiftRichProjection(rich, (address) => {
    const col = deleteIndex(address.col, colIndex, count);
    return col === null ? null : { row: address.row, col };
  }, (drawing) => deleteDrawingColumns(drawing, colIndex, count), keepIndex, (col) => (
    deleteIndex(col, colIndex, count)
  ));
}

function shiftRichProjection(
  rich: ReadOnlyRichProjection | undefined,
  mapAddress: (address: CellAddress) => CellAddress | null,
  mapDrawing: (drawing: DrawingProjection) => DrawingProjection | null,
  mapRow: (row: number) => number | null,
  mapColumn: (column: number) => number | null
): ReadOnlyRichProjection {
  const source = { ...defaultRichProjection(), ...(rich ?? {}) };
  return {
    ...source,
    cellFormats: shiftCellMap(source.cellFormats, mapAddress),
    cellStyles: shiftCellMap(source.cellStyles, mapAddress),
    hiddenRows: shiftIndexList(source.hiddenRows, mapRow),
    hiddenColumns: shiftIndexList(source.hiddenColumns, mapColumn),
    freezePane: shiftFreezePane(source.freezePane, mapAddress),
    hyperlinks: shiftCellMap(source.hyperlinks, mapAddress),
    drawings: (source.drawings ?? []).flatMap((drawing) => {
      const shifted = mapDrawing(drawing);
      return shifted ? [shifted] : [];
    }),
  };
}

function keepIndex(index: number): number {
  return index;
}

function shiftCellMap<T>(
  values: Record<string, T> | undefined,
  mapAddress: (address: CellAddress) => CellAddress | null
): Record<string, T> {
  const shifted: Record<string, T> = {};
  for (const [key, value] of Object.entries(values ?? {})) {
    const address = parseCellKey(key);
    const nextAddress = address ? mapAddress(address) : null;
    if (nextAddress) {
      shifted[cellKey(nextAddress.row, nextAddress.col)] = value;
    } else if (!address) {
      shifted[key] = value;
    }
  }
  return shifted;
}

function shiftIndexList(
  values: number[] | undefined,
  mapIndex: (index: number) => number | null
): number[] {
  const shifted = new Set<number>();
  for (const value of values ?? []) {
    const next = mapIndex(value);
    if (next !== null) {
      shifted.add(next);
    }
  }
  return Array.from(shifted).sort((left, right) => left - right);
}

function shiftFreezePane(
  freezePane: ReadOnlyRichProjection["freezePane"] | undefined,
  mapAddress: (address: CellAddress) => CellAddress | null
): ReadOnlyRichProjection["freezePane"] | undefined {
  if (!freezePane) return undefined;
  const address = parseCellKey(freezePane.topLeftCell);
  if (!address) return freezePane;
  const nextAddress = mapAddress(address);
  if (!nextAddress) return undefined;
  return {
    ...freezePane,
    topLeftCell: cellKey(nextAddress.row, nextAddress.col),
  };
}

function shiftDrawingRows(
  drawing: DrawingProjection,
  rowIndex: number,
  count: number
): DrawingProjection {
  return {
    ...drawing,
    fromRow: drawing.fromRow >= rowIndex ? drawing.fromRow + count : drawing.fromRow,
    toRow: drawing.toRow != null && drawing.toRow >= rowIndex
      ? drawing.toRow + count
      : optionalAnchor(drawing.toRow),
  };
}

function shiftDrawingColumns(
  drawing: DrawingProjection,
  colIndex: number,
  count: number
): DrawingProjection {
  return {
    ...drawing,
    fromCol: drawing.fromCol >= colIndex ? drawing.fromCol + count : drawing.fromCol,
    toCol: drawing.toCol != null && drawing.toCol >= colIndex
      ? drawing.toCol + count
      : optionalAnchor(drawing.toCol),
  };
}

function deleteDrawingRows(
  drawing: DrawingProjection,
  rowIndex: number,
  count: number
): DrawingProjection | null {
  const fromRow = deleteAnchorIndex(drawing.fromRow, rowIndex, count);
  const toRow = drawing.toRow == null
    ? undefined
    : deleteAnchorIndex(drawing.toRow, rowIndex, count);
  if (fromRow === null && (toRow === null || toRow === undefined)) return null;
  return {
    ...drawing,
    fromRow: fromRow ?? rowIndex,
    toRow: toRow === null ? fromRow ?? rowIndex : toRow,
  };
}

function deleteDrawingColumns(
  drawing: DrawingProjection,
  colIndex: number,
  count: number
): DrawingProjection | null {
  const fromCol = deleteAnchorIndex(drawing.fromCol, colIndex, count);
  const toCol = drawing.toCol == null
    ? undefined
    : deleteAnchorIndex(drawing.toCol, colIndex, count);
  if (fromCol === null && (toCol === null || toCol === undefined)) return null;
  return {
    ...drawing,
    fromCol: fromCol ?? colIndex,
    toCol: toCol === null ? fromCol ?? colIndex : toCol,
  };
}

function deleteIndex(index: number, start: number, count: number): number | null {
  if (index < start) return index;
  if (index >= start + count) return index - count;
  return null;
}

function deleteAnchorIndex(index: number, start: number, count: number): number | null {
  if (index < start) return index;
  if (index >= start + count) return index - count;
  return null;
}

function optionalAnchor(value: number | null | undefined): number | undefined {
  return value ?? undefined;
}

function columnName(colIndex: number): string {
  let col = colIndex + 1;
  let result = "";
  while (col > 0) {
    const rem = (col - 1) % 26;
    result = String.fromCharCode(65 + rem) + result;
    col = Math.floor((col - 1) / 26);
  }
  return result;
}
