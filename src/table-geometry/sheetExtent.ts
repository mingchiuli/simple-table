import type { CellValue, MergeRange } from "@/types";

export type SheetExtent = {
  rowCount: number;
  columnCount: number;
};

export function calculateSheetExtent(
  rows: CellValue[][],
  merges: MergeRange[],
  columnWidths: Record<number, number> | undefined,
  rowHeights: Record<number, number> | undefined
): SheetExtent {
  const valueRowCount = rows.length;
  const valueColumnCount = rows.reduce((max, row) => Math.max(max, row.length), 0);
  const mergeRowCount = merges.reduce((max, merge) => Math.max(max, merge.endRow + 1), 0);
  const mergeColumnCount = merges.reduce((max, merge) => Math.max(max, merge.endCol + 1), 0);
  const layoutRowCount = recordExtent(rowHeights);
  const layoutColumnCount = recordExtent(columnWidths);

  return {
    rowCount: Math.max(valueRowCount, mergeRowCount, layoutRowCount),
    columnCount: Math.max(valueColumnCount, mergeColumnCount, layoutColumnCount),
  };
}

function recordExtent(record: Record<number, number> | undefined): number {
  if (!record) return 0;
  return Object.keys(record).reduce((max, key) => Math.max(max, Number(key) + 1), 0);
}
