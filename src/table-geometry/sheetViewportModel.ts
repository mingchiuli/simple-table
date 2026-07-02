import type { CellValue, MergeRange, SheetRichProjection } from "@/types";

export type SheetViewportModel = {
  rows: CellValue[][];
  columns: string[];
  merges: MergeRange[];
  columnWidths?: Record<number, number>;
  rowHeights?: Record<number, number>;
  rich?: SheetRichProjection;
};

export function createSheetViewportModel(model: SheetViewportModel): SheetViewportModel {
  return model;
}
