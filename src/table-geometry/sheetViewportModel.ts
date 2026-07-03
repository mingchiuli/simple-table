import type {
  CellFormatProjection,
  CellStyleProjection,
  CellValue,
  MergeRange,
  SheetRichProjection,
} from "@/types";
import { calculateSheetExtent, type SheetExtent } from "@/table-geometry/sheetExtent";

export type SheetViewportModel = {
  rows: CellValue[][];
  columns: string[];
  merges: MergeRange[];
  columnWidths: Record<number, number>;
  rowHeights: Record<number, number>;
  rich: SheetRichProjection;
  extent: SheetExtent;
  cellAt: (rowIndex: number, colIndex: number) => CellValue | undefined;
  formatAt: (rowIndex: number, colIndex: number) => CellFormatProjection | undefined;
  styleAt: (rowIndex: number, colIndex: number) => CellStyleProjection | undefined;
};

export type SheetViewportSource = {
  rows: CellValue[][];
  columns: string[];
  merges?: MergeRange[];
  columnWidths?: Record<number, number>;
  rowHeights?: Record<number, number>;
  rich?: SheetRichProjection;
};

export function createSheetViewportModel(source: SheetViewportSource): SheetViewportModel {
  const rows = source.rows;
  const columns = source.columns;
  const merges = source.merges ?? [];
  const columnWidths = { ...(source.columnWidths ?? {}) };
  const rowHeights = { ...(source.rowHeights ?? {}) };
  const rich = source.rich ?? {};
  const extent = calculateSheetExtent(rows, merges, columnWidths, rowHeights);

  return {
    rows,
    columns,
    merges,
    columnWidths,
    rowHeights,
    rich,
    extent,
    cellAt: (rowIndex, colIndex) => rows[rowIndex]?.[colIndex],
    formatAt: (rowIndex, colIndex) => {
      const key = excelCellKey(rowIndex, colIndex);
      const explicit = rich.cellFormats?.[key];
      const style = rich.cellStyles?.[key];
      if (!explicit && !style?.numberFormat) return undefined;
      return {
        ...explicit,
        numberFormat: explicit?.numberFormat ?? style?.numberFormat,
      };
    },
    styleAt: (rowIndex, colIndex) => rich.cellStyles?.[excelCellKey(rowIndex, colIndex)],
  };
}

function excelCellKey(rowIndex: number, colIndex: number): string {
  let col = colIndex + 1;
  let letters = "";
  while (col > 0) {
    const rem = (col - 1) % 26;
    letters = String.fromCharCode(65 + rem) + letters;
    col = Math.floor((col - 1) / 26);
  }
  return `${letters}${rowIndex + 1}`;
}
