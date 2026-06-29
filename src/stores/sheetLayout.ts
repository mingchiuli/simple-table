import type { FileData } from "@/types";

export const useSheetLayoutStore = defineStore("sheetLayout", {
  state: () => ({
    sheetColumnWidths: {} as Record<number, Record<number, number>>,
    sheetRowHeights: {} as Record<number, Record<number, number>>,
  }),
  actions: {
    reset() {
      this.sheetColumnWidths = {};
      this.sheetRowHeights = {};
    },
    syncFromData(data: FileData | null) {
      if (!data) {
        this.reset();
        return;
      }

      this.sheetColumnWidths = Object.fromEntries(
        data.sheets
          .map((sheet, index) => [index, sheet.columnWidths ?? {}] as const)
          .filter(([, widths]) => Object.keys(widths).length > 0)
      );
      this.sheetRowHeights = Object.fromEntries(
        data.sheets
          .map((sheet, index) => [index, sheet.rowHeights ?? {}] as const)
          .filter(([, heights]) => Object.keys(heights).length > 0)
      );
    },
    setColumnWidth(sheetIndex: number, colIndex: number, width: number | undefined) {
      this.sheetColumnWidths = patchNestedNumberRecord(this.sheetColumnWidths, sheetIndex, colIndex, width);
    },
    setRowHeight(sheetIndex: number, rowIndex: number, height: number | undefined) {
      this.sheetRowHeights = patchNestedNumberRecord(this.sheetRowHeights, sheetIndex, rowIndex, height);
    },
  },
});

function patchNestedNumberRecord(
  current: Record<number, Record<number, number>>,
  sheetIndex: number,
  key: number,
  value: number | undefined
): Record<number, Record<number, number>> {
  const sheetRecord = { ...(current[sheetIndex] ?? {}) };
  if (value === undefined) {
    delete sheetRecord[key];
  } else {
    sheetRecord[key] = value;
  }

  const next = { ...current };
  if (Object.keys(sheetRecord).length) {
    next[sheetIndex] = sheetRecord;
  } else {
    delete next[sheetIndex];
  }
  return next;
}
