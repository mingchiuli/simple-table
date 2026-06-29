import type { CellValue, FileData, SearchResult } from '@/types';

type CellPosition = { row: number; col: number };

export type PendingCellChange = {
  sheetIndex: number;
  row: number;
  col: number;
  value: string;
  oldValue: CellValue;
};

export const useDocumentUiStore = defineStore('documentUi', {
  state: () => ({
    currentSheetIndex: 0,
    selectedCell: null as CellPosition | null,
    cellEditorValue: '',
    autoScroll: false,
    searchResults: [] as SearchResult[],
    searchQuery: '',
    isSearching: false,
    sheetSelectedCells: new Map<number, CellPosition>(),
    draftCellValues: new Map<string, string>(),
    pendingCellChanges: new Map<string, PendingCellChange>(),
    inFlightCellChanges: new Map<string, PendingCellChange>(),
    sheetColumnWidths: {} as Record<number, Record<number, number>>,
    sheetRowHeights: {} as Record<number, Record<number, number>>,
  }),
  actions: {
    resetForDocument(fileData: FileData | null) {
      this.currentSheetIndex = 0;
      this.selectedCell = null;
      this.cellEditorValue = '';
      this.autoScroll = false;
      this.searchResults = [];
      this.searchQuery = '';
      this.isSearching = false;
      this.sheetSelectedCells = new Map();
      this.resetPendingEdits();
      this.hydrateLayout(fileData);
    },
    resetPendingEdits() {
      this.draftCellValues.clear();
      this.pendingCellChanges.clear();
      this.inFlightCellChanges.clear();
    },
    hydrateLayout(fileData: FileData | null) {
      if (!fileData) {
        this.sheetColumnWidths = {};
        this.sheetRowHeights = {};
        return;
      }

      this.sheetColumnWidths = Object.fromEntries(
        fileData.sheets
          .map((sheet, index) => [index, sheet.columnWidths ?? {}] as const)
          .filter(([, widths]) => Object.keys(widths).length > 0)
      );
      this.sheetRowHeights = Object.fromEntries(
        fileData.sheets
          .map((sheet, index) => [index, sheet.rowHeights ?? {}] as const)
          .filter(([, heights]) => Object.keys(heights).length > 0)
      );
    },
    selectCell(row: number, col: number, autoScroll = false) {
      this.autoScroll = autoScroll;
      this.selectedCell = { row, col };
    },
    clearSelection() {
      this.selectedCell = null;
      this.cellEditorValue = '';
    },
    rememberCurrentSheetSelection() {
      if (this.selectedCell) {
        this.sheetSelectedCells.set(this.currentSheetIndex, this.selectedCell);
      }
    },
    restoreSheetSelection(sheetIndex: number, editorValueFor: (cell: CellPosition) => string) {
      this.currentSheetIndex = sheetIndex;
      const savedCell = this.sheetSelectedCells.get(sheetIndex);
      if (!savedCell) {
        this.clearSelection();
        return;
      }
      this.selectedCell = savedCell;
      this.cellEditorValue = editorValueFor(savedCell);
      this.autoScroll = true;
    },
    setColumnWidth(sheetIndex: number, colIndex: number, width: number | undefined) {
      this.sheetColumnWidths = patchNestedNumberRecord(this.sheetColumnWidths, sheetIndex, colIndex, width);
    },
    setRowHeight(sheetIndex: number, rowIndex: number, height: number | undefined) {
      this.sheetRowHeights = patchNestedNumberRecord(this.sheetRowHeights, sheetIndex, rowIndex, height);
    },
    clearSearch() {
      this.searchResults = [];
      this.searchQuery = '';
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
