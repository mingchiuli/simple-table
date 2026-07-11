import type { CellValue, MergeRange, ReadOnlyRichProjection, SheetExtent } from "@/types";
import { parseCellKey } from "@/utils/cellAddress";

export type { SheetExtent } from "@/types";

export function calculateSheetExtent(
  rows: CellValue[][],
  merges: MergeRange[],
  columnWidths: Record<number, number> | undefined,
  rowHeights: Record<number, number> | undefined,
  rich?: ReadOnlyRichProjection
): SheetExtent {
  const valueRowCount = rows.length;
  const valueColumnCount = rows.reduce((max, row) => Math.max(max, row.length), 0);
  const mergeRowCount = merges.reduce((max, merge) => Math.max(max, merge.endRow + 1), 0);
  const mergeColumnCount = merges.reduce((max, merge) => Math.max(max, merge.endCol + 1), 0);
  const layoutRowCount = recordExtent(rowHeights);
  const layoutColumnCount = recordExtent(columnWidths);
  const richExtent = calculateRichExtent(rich);

  return {
    rowCount: Math.max(valueRowCount, mergeRowCount, layoutRowCount, richExtent.rowCount),
    columnCount: Math.max(
      valueColumnCount,
      mergeColumnCount,
      layoutColumnCount,
      richExtent.columnCount
    ),
  };
}

function recordExtent(record: Record<number, number> | undefined): number {
  if (!record) return 0;
  return Object.keys(record).reduce((max, key) => Math.max(max, Number(key) + 1), 0);
}

function calculateRichExtent(rich: ReadOnlyRichProjection | undefined): SheetExtent {
  if (!rich) return { rowCount: 0, columnCount: 0 };

  let rowCount = 0;
  let columnCount = 0;
  const includeCellKey = (key: string) => {
    const address = parseCellKey(key);
    if (!address) return;
    rowCount = Math.max(rowCount, address.row + 1);
    columnCount = Math.max(columnCount, address.col + 1);
  };

  Object.keys(rich.cellFormats ?? {}).forEach(includeCellKey);
  Object.keys(rich.cellStyles ?? {}).forEach(includeCellKey);
  Object.keys(rich.hyperlinks ?? {}).forEach(includeCellKey);

  for (const row of rich.hiddenRows ?? []) {
    rowCount = Math.max(rowCount, row + 1);
  }
  for (const column of rich.hiddenColumns ?? []) {
    columnCount = Math.max(columnCount, column + 1);
  }
  for (const drawing of rich.drawings ?? []) {
    rowCount = Math.max(rowCount, drawing.fromRow + 1, (drawing.toRow ?? drawing.fromRow) + 1);
    columnCount = Math.max(
      columnCount,
      drawing.fromCol + 1,
      (drawing.toCol ?? drawing.fromCol) + 1
    );
  }

  return { rowCount, columnCount };
}
