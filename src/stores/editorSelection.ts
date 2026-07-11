import type { EditorPatch } from "@/types";

type CellPosition = { row: number; col: number };

export type EditorSelectionSnapshot = {
  currentSheetIndex: number;
  selectedCell: CellPosition | null;
  cellEditorValue: string;
  autoScroll: boolean;
  sheetSelectedCells: Map<number, CellPosition>;
};

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
    captureSnapshot(): EditorSelectionSnapshot {
      return {
        currentSheetIndex: this.currentSheetIndex,
        selectedCell: cloneCellPosition(this.selectedCell),
        cellEditorValue: this.cellEditorValue,
        autoScroll: this.autoScroll,
        sheetSelectedCells: cloneSelectedCells(this.sheetSelectedCells),
      };
    },
    restoreSnapshot(snapshot: EditorSelectionSnapshot) {
      this.currentSheetIndex = snapshot.currentSheetIndex;
      this.selectedCell = cloneCellPosition(snapshot.selectedCell);
      this.cellEditorValue = snapshot.cellEditorValue;
      this.autoScroll = snapshot.autoScroll;
      this.sheetSelectedCells = cloneSelectedCells(snapshot.sheetSelectedCells);
    },
    setEditorValue(value: string) {
      this.cellEditorValue = value;
    },
    activateSheet(sheetIndex: number) {
      this.currentSheetIndex = sheetIndex;
      this.clearSelection();
    },
    switchSheet(sheetIndex: number, editorValueFor: (cell: CellPosition) => string) {
      this.rememberCurrentSheetSelection();
      this.restoreSheetSelection(sheetIndex, editorValueFor);
    },
    selectCell(row: number, col: number, autoScroll = false) {
      this.autoScroll = autoScroll;
      this.selectedCell = { row, col };
    },
    focusSearchResult(sheetIndex: number, row: number, col: number, editorValue: string) {
      this.currentSheetIndex = sheetIndex;
      this.selectCell(row, col, true);
      this.setEditorValue(editorValue);
    },
    clearSelection() {
      this.selectedCell = null;
      this.cellEditorValue = "";
    },
    rememberCurrentSheetSelection() {
      if (this.selectedCell) {
        this.sheetSelectedCells.set(this.currentSheetIndex, { ...this.selectedCell });
      }
    },
    restoreSheetSelection(sheetIndex: number, editorValueFor: (cell: CellPosition) => string) {
      this.currentSheetIndex = sheetIndex;
      const savedCell = this.sheetSelectedCells.get(sheetIndex);
      if (!savedCell) {
        this.clearSelection();
        return;
      }
      this.selectedCell = { ...savedCell };
      this.cellEditorValue = editorValueFor(savedCell);
      this.autoScroll = true;
    },
    applyEditorPatches(patches: EditorPatch[] | undefined) {
      for (const patch of patches ?? []) {
        switch (patch.type) {
          case "SheetInserted":
            this.shiftSheetSelectionsOnInsert(patch.data.patch.sheetIndex);
            break;
          case "SheetDeleted":
            this.shiftSheetSelectionsOnDelete(patch.data.patch.sheetIndex);
            break;
          case "SheetsReplaced":
            this.clearSelectionsFromSheet(patch.data.patch.startIndex);
            break;
          case "RowInserted":
            this.shiftRowSelectionsOnInsert(
              patch.data.patch.sheetIndex,
              patch.data.patch.rowIndex,
              patch.data.patch.count
            );
            break;
          case "RowDeleted":
            this.shiftRowSelectionsOnDelete(
              patch.data.patch.sheetIndex,
              patch.data.patch.rowIndex,
              patch.data.patch.count
            );
            break;
          case "ColumnInserted":
            this.shiftColumnSelectionsOnInsert(
              patch.data.patch.sheetIndex,
              patch.data.patch.colIndex,
              patch.data.patch.count
            );
            break;
          case "ColumnDeleted":
            this.shiftColumnSelectionsOnDelete(
              patch.data.patch.sheetIndex,
              patch.data.patch.colIndex,
              patch.data.patch.count
            );
            break;
          default:
            break;
        }
      }
    },
    shiftSheetSelectionsOnInsert(sheetIndex: number) {
      if (this.currentSheetIndex >= sheetIndex) {
        this.currentSheetIndex += 1;
      }
      this.sheetSelectedCells = remapSheetSelections(this.sheetSelectedCells, (index) =>
        index >= sheetIndex ? index + 1 : index
      );
    },
    shiftSheetSelectionsOnDelete(sheetIndex: number) {
      if (this.currentSheetIndex === sheetIndex) {
        this.currentSheetIndex = Math.max(0, sheetIndex - 1);
        this.clearSelection();
      } else if (this.currentSheetIndex > sheetIndex) {
        this.currentSheetIndex -= 1;
      }
      this.sheetSelectedCells = remapSheetSelections(this.sheetSelectedCells, (index) => {
        if (index === sheetIndex) return null;
        return index > sheetIndex ? index - 1 : index;
      });
    },
    clearSelectionsFromSheet(startIndex: number) {
      if (this.currentSheetIndex >= startIndex) {
        this.clearSelection();
      }
      this.sheetSelectedCells = remapSheetSelections(this.sheetSelectedCells, (index) =>
        index >= startIndex ? null : index
      );
    },
    shiftRowSelectionsOnInsert(sheetIndex: number, rowIndex: number, count: number) {
      this.transformSelectionsForSheet(sheetIndex, (cell) =>
        cell.row >= rowIndex ? { ...cell, row: cell.row + count } : cell
      );
    },
    shiftRowSelectionsOnDelete(sheetIndex: number, rowIndex: number, count: number) {
      const end = rowIndex + count;
      this.transformSelectionsForSheet(sheetIndex, (cell) => {
        if (cell.row >= rowIndex && cell.row < end) return null;
        return cell.row >= end ? { ...cell, row: cell.row - count } : cell;
      });
    },
    shiftColumnSelectionsOnInsert(sheetIndex: number, colIndex: number, count: number) {
      this.transformSelectionsForSheet(sheetIndex, (cell) =>
        cell.col >= colIndex ? { ...cell, col: cell.col + count } : cell
      );
    },
    shiftColumnSelectionsOnDelete(sheetIndex: number, colIndex: number, count: number) {
      const end = colIndex + count;
      this.transformSelectionsForSheet(sheetIndex, (cell) => {
        if (cell.col >= colIndex && cell.col < end) return null;
        return cell.col >= end ? { ...cell, col: cell.col - count } : cell;
      });
    },
    transformSelectionsForSheet(
      sheetIndex: number,
      transform: (cell: CellPosition) => CellPosition | null
    ) {
      if (this.currentSheetIndex === sheetIndex && this.selectedCell) {
        const selectedCell = transform(this.selectedCell);
        if (selectedCell) {
          this.selectedCell = selectedCell;
        } else {
          this.clearSelection();
        }
      }

      const rememberedCell = this.sheetSelectedCells.get(sheetIndex);
      if (!rememberedCell) return;
      const transformed = transform(rememberedCell);
      if (transformed) {
        this.sheetSelectedCells.set(sheetIndex, transformed);
      } else {
        this.sheetSelectedCells.delete(sheetIndex);
      }
    },
    clampToSheetData(
      sheetCount: number,
      isCellInSheetBounds: (sheetIndex: number, row: number, col: number) => boolean
    ) {
      if (sheetCount <= 0) {
        this.clearSelection();
        this.currentSheetIndex = 0;
        return;
      }
      if (this.currentSheetIndex >= sheetCount) {
        this.currentSheetIndex = sheetCount - 1;
      }
      for (const [sheetIndex, cell] of this.sheetSelectedCells) {
        if (
          sheetIndex >= sheetCount
          || !isCellInSheetBounds(sheetIndex, cell.row, cell.col)
        ) {
          this.sheetSelectedCells.delete(sheetIndex);
        }
      }
      if (!this.selectedCell) return;

      if (!isCellInSheetBounds(
        this.currentSheetIndex,
        this.selectedCell.row,
        this.selectedCell.col
      )) {
        this.clearSelection();
      }
    },
  },
});

function remapSheetSelections(
  selections: Map<number, CellPosition>,
  mapIndex: (index: number) => number | null
): Map<number, CellPosition> {
  const next = new Map<number, CellPosition>();
  for (const [index, cell] of selections) {
    const mappedIndex = mapIndex(index);
    if (mappedIndex !== null) {
      next.set(mappedIndex, { ...cell });
    }
  }
  return next;
}

function cloneCellPosition(cell: CellPosition | null): CellPosition | null {
  return cell ? { ...cell } : null;
}

function cloneSelectedCells(cells: Map<number, CellPosition>): Map<number, CellPosition> {
  return new Map(Array.from(cells, ([sheetIndex, cell]) => [sheetIndex, { ...cell }]));
}
