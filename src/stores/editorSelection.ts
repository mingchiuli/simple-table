import type { SelectionTransform } from '@/types/editorRuntime';

type CellPosition = { row: number; col: number };

export type EditorSelectionSnapshot = {
  currentSheetIndex: number;
  selectedCell: CellPosition | null;
  cellEditorValue: string;
  autoScroll: boolean;
  sheetSelectedCells: Record<string, CellPosition>;
};

export const useEditorSelectionStore = defineStore("editorSelection", {
  state: () => ({
    currentSheetIndex: 0,
    selectedCell: null as CellPosition | null,
    cellEditorValue: "",
    autoScroll: false,
    sheetSelectedCells: {} as Record<string, CellPosition>,
  }),
  actions: {
    reset() {
      this.currentSheetIndex = 0;
      this.selectedCell = null;
      this.cellEditorValue = "";
      this.autoScroll = false;
      this.sheetSelectedCells = {};
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
        this.sheetSelectedCells[this.currentSheetIndex] = { ...this.selectedCell };
      }
    },
    restoreSheetSelection(sheetIndex: number, editorValueFor: (cell: CellPosition) => string) {
      this.currentSheetIndex = sheetIndex;
      const savedCell = this.sheetSelectedCells[sheetIndex];
      if (!savedCell) {
        this.clearSelection();
        return;
      }
      this.selectedCell = { ...savedCell };
      this.cellEditorValue = editorValueFor(savedCell);
      this.autoScroll = true;
    },
    applySelectionTransforms(transforms: SelectionTransform[]) {
      for (const transform of transforms) {
        switch (transform.type) {
          case 'sheetInserted':
            this.shiftSheetSelectionsOnInsert(transform.sheetIndex);
            break;
          case 'sheetDeleted':
            this.shiftSheetSelectionsOnDelete(transform.sheetIndex);
            break;
          case 'sheetsReplaced':
            this.clearSelectionsFromSheet(transform.startIndex);
            break;
          case 'rowInserted':
            this.shiftRowSelectionsOnInsert(
              transform.sheetIndex,
              transform.rowIndex,
              transform.count,
            );
            break;
          case 'rowDeleted':
            this.shiftRowSelectionsOnDelete(
              transform.sheetIndex,
              transform.rowIndex,
              transform.count,
            );
            break;
          case 'columnInserted':
            this.shiftColumnSelectionsOnInsert(
              transform.sheetIndex,
              transform.colIndex,
              transform.count,
            );
            break;
          case 'columnDeleted':
            this.shiftColumnSelectionsOnDelete(
              transform.sheetIndex,
              transform.colIndex,
              transform.count,
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

      const rememberedCell = this.sheetSelectedCells[sheetIndex];
      if (!rememberedCell) return;
      const transformed = transform(rememberedCell);
      if (transformed) {
        this.sheetSelectedCells[sheetIndex] = transformed;
      } else {
        delete this.sheetSelectedCells[sheetIndex];
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
      for (const [sheetIndexKey, cell] of Object.entries(this.sheetSelectedCells)) {
        const sheetIndex = Number(sheetIndexKey);
        if (
          sheetIndex >= sheetCount
          || !isCellInSheetBounds(sheetIndex, cell.row, cell.col)
        ) {
          delete this.sheetSelectedCells[sheetIndex];
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
  selections: Record<string, CellPosition>,
  mapIndex: (index: number) => number | null
): Record<string, CellPosition> {
  const next: Record<string, CellPosition> = {};
  for (const [indexKey, cell] of Object.entries(selections)) {
    const index = Number(indexKey);
    const mappedIndex = mapIndex(index);
    if (mappedIndex !== null) {
      next[mappedIndex] = { ...cell };
    }
  }
  return next;
}

function cloneCellPosition(cell: CellPosition | null): CellPosition | null {
  return cell ? { ...cell } : null;
}

function cloneSelectedCells(
  cells: Record<string, CellPosition>
): Record<string, CellPosition> {
  return Object.fromEntries(
    Object.entries(cells).map(([sheetIndex, cell]) => [sheetIndex, { ...cell }])
  );
}
