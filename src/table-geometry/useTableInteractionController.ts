import type { ComputedRef } from "vue";
import type { CellValue } from "@/types";
import { cellToDisplayString, cellToEditorString } from "@/composables/usePendingCellSave";
import { useCellEditing } from "@/table-geometry/useCellEditing";

type CellPosition = { row: number; col: number };

type ScrollGeometry = {
  getRowOffset: (rowIndex: number) => number;
  getColumnOffset: (colIndex: number) => number;
  getRowHeight: (rowIndex: number) => number;
  getColumnWidth: (colIndex: number) => number;
  viewportWidth: ComputedRef<number>;
  viewportHeight: ComputedRef<number>;
};

type UseTableInteractionControllerOptions = {
  data: ComputedRef<CellValue[][]>;
  selectedCell: ComputedRef<CellPosition | null | undefined>;
  autoScroll: ComputedRef<boolean | undefined>;
  getDraftValue: (rowIndex: number, colIndex: number) => string | undefined;
  normalizeCellPosition: (rowIndex: number, colIndex: number) => {
    rowIndex: number;
    colIndex: number;
  };
  scrollCellIntoView: (
    cell: CellPosition | null | undefined,
    enabled: boolean | undefined,
    geometry: {
      getRowOffset: (rowIndex: number) => number;
      getColumnOffset: (colIndex: number) => number;
      getRowHeight: (rowIndex: number) => number;
      getColumnWidth: (colIndex: number) => number;
      viewportWidth: number;
      viewportHeight: number;
    }
  ) => void;
  scrollGeometry: ScrollGeometry;
  emitSelectCell: (rowIndex: number, colIndex: number) => void;
  emitEditing: (rowIndex: number, colIndex: number, value: string) => void;
  emitChange: (rowIndex: number, colIndex: number, value: string) => void;
  emitCancel: (rowIndex: number, colIndex: number) => void;
};

export function useTableInteractionController({
  data,
  selectedCell,
  autoScroll,
  getDraftValue,
  normalizeCellPosition,
  scrollCellIntoView,
  scrollGeometry,
  emitSelectCell,
  emitEditing,
  emitChange,
  emitCancel,
}: UseTableInteractionControllerOptions) {
  const {
    editingValue,
    isManualClick,
    isEditing,
    beginEdit,
    resetEditing,
    handleInput,
    commit,
    cancel,
    syncSelectedCell,
  } = useCellEditing({
    getCellKey,
    getInitialValue: (rowIndex, colIndex) =>
      getDraftValue(rowIndex, colIndex) ?? getCellEditorValue(rowIndex, colIndex),
    emitEditing,
    emitChange,
    emitCancel,
  });

  watch(data, () => {
    const cell = selectedCell.value;
    if (!cell) return;

    const key = getCellKey(cell.row, cell.col);
    if (editingValue.value[key] === undefined) return;

    editingValue.value[key] = getDraftValue(cell.row, cell.col)
      ?? getCellEditorValue(cell.row, cell.col);
  }, { deep: true });

  watch(selectedCell, (newCell) => {
    if (!newCell) {
      resetEditing();
      return;
    }

    syncSelectedCell(getCellKey(newCell.row, newCell.col));
    scrollCellIntoView(newCell, autoScroll.value, {
      getRowOffset: scrollGeometry.getRowOffset,
      getColumnOffset: scrollGeometry.getColumnOffset,
      getRowHeight: scrollGeometry.getRowHeight,
      getColumnWidth: scrollGeometry.getColumnWidth,
      viewportWidth: scrollGeometry.viewportWidth.value,
      viewportHeight: scrollGeometry.viewportHeight.value,
    });
  }, { deep: true });

  function getCellKey(rowIndex: number, colIndex: number): string {
    return `${rowIndex}-${colIndex}`;
  }

  function getCellEditorValue(rowIndex: number, colIndex: number): string {
    return cellToEditorString(data.value[rowIndex]?.[colIndex]);
  }

  function getDisplayValue(rowIndex: number, colIndex: number, cellValue: CellValue | undefined): string {
    const key = getCellKey(rowIndex, colIndex);
    if (editingValue.value[key] !== undefined) return editingValue.value[key];

    const draftValue = getDraftValue(rowIndex, colIndex);
    if (draftValue !== undefined) return draftValue;

    return cellToDisplayString(cellValue);
  }

  function handleCommit(rowIndex: number, colIndex: number, value: string) {
    const originalValue = getCellEditorValue(rowIndex, colIndex);
    if (value !== originalValue || getDraftValue(rowIndex, colIndex) !== undefined) {
      commit(rowIndex, colIndex, value);
    } else {
      resetEditing();
    }
  }

  function handleCancel(rowIndex: number, colIndex: number) {
    cancel(rowIndex, colIndex);
  }

  function handleCellClick(rowIndex: number, colIndex: number) {
    const normalized = normalizeCellPosition(rowIndex, colIndex);
    emitSelectCell(normalized.rowIndex, normalized.colIndex);
  }

  function handleCellDoubleClick(rowIndex: number, colIndex: number) {
    const normalized = normalizeCellPosition(rowIndex, colIndex);
    emitSelectCell(normalized.rowIndex, normalized.colIndex);
    beginEdit(normalized.rowIndex, normalized.colIndex);
  }

  return {
    editingValue,
    isManualClick,
    isEditing,
    getCellKey,
    getDraftValue,
    getDisplayValue,
    handleCellClick,
    handleCellDoubleClick,
    handleInput,
    handleCommit,
    handleCancel,
  };
}
