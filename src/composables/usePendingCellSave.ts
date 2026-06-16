import { computed, onUnmounted, watch, type ComputedRef, type Ref } from 'vue';
import { ElMessage } from 'element-plus';
import * as api from '@/api';
import type { CellValue, FileData, SheetData, SortState } from '@/types';

type CellPosition = { row: number; col: number };

type PendingCellChange = {
  sheetIndex: number;
  row: number;
  col: number;
  value: string;
  oldValue: CellValue;
};

type UsePendingCellSaveOptions = {
  fileData: ComputedRef<FileData | null>;
  currentSheet: ComputedRef<SheetData | null>;
  currentSheetIndex: Ref<number>;
  selectedCell: Ref<CellPosition | null>;
  cellEditorValue: Ref<string>;
  currentSortColumn: Ref<SortState | null>;
  refreshEditorState: () => Promise<void>;
  markPendingContentChange: () => void;
  clearPendingContentChange: () => void;
};

export function cellToEditorString(value: CellValue | undefined): string {
  return value === null || value === undefined ? '' : String(value);
}

function parseCellValue(value: string): CellValue {
  if (value === '') return null;
  if (/^0\d/.test(value)) return value;
  if (/^-?\d+$/.test(value)) {
    const num = Number(value);
    return Number.isSafeInteger(num) ? num : value;
  }
  if (/^-?\d+\.\d+$/.test(value)) {
    const num = Number(value);
    return Number.isFinite(num) ? num : value;
  }
  if (value.toLowerCase() === 'true') return true;
  if (value.toLowerCase() === 'false') return false;
  return value;
}

function getCellKey(sheetIndex: number, row: number, col: number) {
  return `${sheetIndex},${row},${col}`;
}

export function usePendingCellSave({
  fileData,
  currentSheet,
  currentSheetIndex,
  selectedCell,
  cellEditorValue,
  currentSortColumn,
  refreshEditorState,
  markPendingContentChange,
  clearPendingContentChange,
}: UsePendingCellSaveOptions) {
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  const pendingChanges = new Map<string, PendingCellChange>();
  let pendingSavePromise: Promise<boolean> | null = null;

  const currentCellValue = computed(() => {
    if (!selectedCell.value || !currentSheet.value) return undefined;
    return currentSheet.value.rows[selectedCell.value.row]?.[selectedCell.value.col];
  });

  watch(selectedCell, (newCell) => {
    if (newCell && currentSheet.value) {
      cellEditorValue.value = cellToEditorString(currentSheet.value.rows[newCell.row]?.[newCell.col]);
    } else {
      cellEditorValue.value = '';
    }
  }, { immediate: true });

  watch(currentCellValue, (newValue) => {
    if (selectedCell.value) {
      cellEditorValue.value = cellToEditorString(newValue);
    }
  });

  watch(cellEditorValue, (newValue) => {
    if (!selectedCell.value || !currentSheet.value) return;

    const { row, col } = selectedCell.value;
    const originalValue = currentSheet.value.rows[row]?.[col] ?? null;
    if (newValue === cellToEditorString(originalValue)) return;

    currentSheet.value.rows[row][col] = newValue;
    queueCellChange(currentSheetIndex.value, row, col, newValue, originalValue);
    markPendingContentChange();
    schedulePendingSave();
  });

  function queueCellChange(sheetIndex: number, row: number, col: number, value: string, oldValue: CellValue) {
    const key = getCellKey(sheetIndex, row, col);
    const existing = pendingChanges.get(key);
    pendingChanges.set(key, {
      sheetIndex,
      row,
      col,
      value,
      oldValue: existing?.oldValue ?? oldValue,
    });
  }

  function schedulePendingSave() {
    if (debounceTimer) {
      clearTimeout(debounceTimer);
    }
    debounceTimer = setTimeout(() => {
      debounceTimer = null;
      pendingSavePromise = debouncedSave().finally(() => {
        pendingSavePromise = null;
      });
    }, 500);
  }

  async function commitCellChange(
    sheetIndex: number,
    rowIndex: number,
    colIndex: number,
    value: string,
    oldValueOverride?: CellValue
  ) {
    if (!fileData.value) return;

    const sheet = fileData.value.sheets[sheetIndex];
    if (!sheet) return;

    currentSortColumn.value = null;
    const oldValue = oldValueOverride ?? sheet.rows[rowIndex][colIndex];
    const newValue = parseCellValue(value);
    const isCurrentCell = currentSheetIndex.value === sheetIndex
      && selectedCell.value?.row === rowIndex
      && selectedCell.value?.col === colIndex;

    await api.setCell(sheetIndex, rowIndex, colIndex, oldValue, newValue);

    if (sheet.rows[rowIndex]) {
      sheet.rows[rowIndex][colIndex] = newValue;
    }

    if (isCurrentCell) {
      cellEditorValue.value = value;
    }

    await refreshEditorState();
  }

  async function debouncedSave(): Promise<boolean> {
    if (!pendingChanges.size) {
      clearPendingContentChange();
      return true;
    }

    const changes = Array.from(pendingChanges.values());
    pendingChanges.clear();

    for (let i = 0; i < changes.length; i += 1) {
      const { sheetIndex, row, col, value, oldValue } = changes[i];
      try {
        await commitCellChange(sheetIndex, row, col, value, oldValue);
      } catch (error) {
        if (fileData.value) {
          for (const change of changes.slice(i)) {
            const sheet = fileData.value.sheets[change.sheetIndex];
            if (sheet?.rows[change.row]) {
              sheet.rows[change.row][change.col] = change.oldValue;
            }
          }
        }
        ElMessage.error(`保存失败: ${error}，已恢复所有更改`);
        if (!pendingChanges.size) {
          clearPendingContentChange();
        }
        return false;
      }
    }
    if (!pendingChanges.size) {
      clearPendingContentChange();
    }
    return true;
  }

  async function flushPendingCellChanges(): Promise<boolean> {
    if (debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }

    while (true) {
      if (pendingSavePromise) {
        const saved = await pendingSavePromise;
        if (!saved) return false;
      } else if (pendingChanges.size) {
        pendingSavePromise = debouncedSave().finally(() => {
          pendingSavePromise = null;
        });
        const saved = await pendingSavePromise;
        if (!saved) return false;
      }

      if (debounceTimer) {
        clearTimeout(debounceTimer);
        debounceTimer = null;
      }
      if (!pendingChanges.size && !pendingSavePromise) {
        return true;
      }
    }
  }

  async function handleCellChange(rowIndex: number, colIndex: number, value: string) {
    if (!currentSheet.value) return;

    const oldValue = currentSheet.value.rows[rowIndex]?.[colIndex] ?? null;

    try {
      await commitCellChange(currentSheetIndex.value, rowIndex, colIndex, value, oldValue);
    } catch (error) {
      if (currentSheet.value?.rows[rowIndex]) {
        currentSheet.value.rows[rowIndex][colIndex] = oldValue;
      }
      ElMessage.error(`Failed to set cell: ${error}`);
    }
  }

  function handleCellEditing(row: number, col: number, value: string) {
    if (selectedCell.value?.row === row && selectedCell.value?.col === col) {
      cellEditorValue.value = value;
    }

    if (!currentSheet.value) return;

    const originalValue = currentSheet.value.rows[row]?.[col] ?? null;
    if (value === cellToEditorString(originalValue)) return;

    currentSheet.value.rows[row][col] = value;
    queueCellChange(currentSheetIndex.value, row, col, value, originalValue);
    markPendingContentChange();
    schedulePendingSave();
  }

  function handleCellEditorSubmit() {
    if (!selectedCell.value || !currentSheet.value) return;

    const { row, col } = selectedCell.value;
    const currentValue = currentSheet.value.rows[row]?.[col] ?? null;
    if (cellEditorValue.value !== cellToEditorString(currentValue)) {
      currentSheet.value.rows[row][col] = cellEditorValue.value;
      queueCellChange(currentSheetIndex.value, row, col, cellEditorValue.value, currentValue);
      markPendingContentChange();
    }
    void flushPendingCellChanges();
  }

  function handleDeselectCell() {
    selectedCell.value = null;
    cellEditorValue.value = '';
  }

  onUnmounted(() => {
    if (debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }
  });

  return {
    cellToEditorString,
    flushPendingCellChanges,
    handleCellChange,
    handleCellEditing,
    handleCellEditorSubmit,
    handleDeselectCell,
  };
}
