type CellPosition = { row: number; col: number };

export const useEditorSelectionStore = defineStore("editorSelection", {
  state: () => ({
    currentSheetIndex: 0,
    selectedCell: null as CellPosition | null,
    cellEditorValue: "",
    autoScroll: false,
    sheetSelectedCells: new Map<number, CellPosition>(),
  }),
  actions: {
    reset() {
      this.currentSheetIndex = 0;
      this.selectedCell = null;
      this.cellEditorValue = "";
      this.autoScroll = false;
      this.sheetSelectedCells = new Map();
    },
    selectCell(row: number, col: number, autoScroll = false) {
      this.autoScroll = autoScroll;
      this.selectedCell = { row, col };
    },
    clearSelection() {
      this.selectedCell = null;
      this.cellEditorValue = "";
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
    clampToSheetData(sheetCount: number, rowLengthAt: (sheetIndex: number, row: number) => number | null) {
      if (sheetCount <= 0) {
        this.clearSelection();
        this.currentSheetIndex = 0;
        return;
      }
      if (this.currentSheetIndex >= sheetCount) {
        this.currentSheetIndex = sheetCount - 1;
      }
      if (!this.selectedCell) return;

      const rowLength = rowLengthAt(this.currentSheetIndex, this.selectedCell.row);
      if (rowLength === null || this.selectedCell.col >= rowLength) {
        this.clearSelection();
      }
    },
  },
});
