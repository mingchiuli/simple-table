import type { CellValue } from "@/types";

export type CellSaveRequest = {
  sheetIndex: number;
  row: number;
  col: number;
  value: string;
  oldValue: CellValue;
};

export const usePendingCellSavesStore = defineStore("pendingCellSaves", {
  state: () => ({
    draftCellValues: new Map<string, string>(),
    queuedCellSaves: new Map<string, CellSaveRequest>(),
    activeCellSaves: new Map<string, CellSaveRequest>(),
  }),
  actions: {
    reset() {
      this.draftCellValues.clear();
      this.queuedCellSaves.clear();
      this.activeCellSaves.clear();
    },
  },
});
