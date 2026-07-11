import type {
  CellFormatProjection,
  CellStyleProjection,
  CellValue,
  MergeRange,
  ReadOnlyRichProjection,
} from "@/types";
import { defaultRichProjection } from "@/types";
import type { SheetExtent } from "@/table-geometry/sheetExtent";
import { cellKey } from "@/utils/cellAddress";

export type SheetViewportModel = {
  columns: string[];
  merges: MergeRange[];
  columnWidths: Record<number, number>;
  rowHeights: Record<number, number>;
  rich: ReadOnlyRichProjection;
  extent: SheetExtent;
  cellAt: (rowIndex: number, colIndex: number) => CellValue | undefined;
  formatAt: (rowIndex: number, colIndex: number) => CellFormatProjection | undefined;
  styleAt: (rowIndex: number, colIndex: number) => CellStyleProjection | undefined;
};

export type SheetViewportSource = {
  cellAt: (rowIndex: number, colIndex: number) => CellValue | undefined;
  columns: string[];
  merges?: MergeRange[];
  columnWidths?: Record<number, number>;
  rowHeights?: Record<number, number>;
  rich?: ReadOnlyRichProjection;
  extent: SheetExtent;
};

export function createSheetViewportModel(source: SheetViewportSource): SheetViewportModel {
  const columns = source.columns;
  const merges = source.merges ?? [];
  const columnWidths = { ...(source.columnWidths ?? {}) };
  const rowHeights = { ...(source.rowHeights ?? {}) };
  const rich = source.rich ?? defaultRichProjection();
  const extent = source.extent;

  return {
    columns,
    merges,
    columnWidths,
    rowHeights,
    rich,
    extent,
    cellAt: source.cellAt,
    formatAt: (rowIndex, colIndex) => {
      const key = cellKey(rowIndex, colIndex);
      const explicit = rich.cellFormats?.[key];
      const style = rich.cellStyles?.[key];
      if (!explicit && !style?.numberFormat) return undefined;
      return {
        ...explicit,
        numberFormat: explicit?.numberFormat ?? style?.numberFormat,
      };
    },
    styleAt: (rowIndex, colIndex) => rich.cellStyles?.[cellKey(rowIndex, colIndex)],
  };
}
