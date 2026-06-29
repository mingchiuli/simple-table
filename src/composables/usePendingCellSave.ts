import type { ComputedRef, Ref } from 'vue';
import * as api from '@/api';
import { useDocumentSessionStore, type PendingCellChange } from '@/stores/documentSession';
import type { CellValue, EditorMutationResponse, FileData, SheetData } from '@/types';
import { getCellKey } from '@/utils/cellKey';

type CellPosition = { row: number; col: number };

type UsePendingCellSaveOptions = {
  fileData: ComputedRef<FileData | null>;
  currentSheet: ComputedRef<SheetData | null>;
  currentSheetIndex: Ref<number>;
  selectedCell: Ref<CellPosition | null>;
  cellEditorValue: Ref<string>;
  applyMutationResponse: (response: EditorMutationResponse) => void;
  markPendingContentChange: () => void;
  clearPendingContentChange: () => void;
};

export function cellToEditorString(value: CellValue | undefined): string {
  if (value === null || value === undefined) return '';
  if (isFormulaCell(value)) return value.formula;
  return String(value);
}

export function cellToDisplayString(value: CellValue | undefined): string {
  if (value === null || value === undefined) return '';
  if (isFormulaCell(value)) {
    return value.error ?? cellToDisplayString(value.cachedValue);
  }
  return String(value);
}

export function isFormulaCell(value: CellValue | undefined): value is Extract<CellValue, { type: 'formula' }> {
  return typeof value === 'object'
    && value !== null
    && !Array.isArray(value)
    && value.type === 'formula';
}

function normalizeFormula(value: string): string {
  return value.startsWith('=') ? value : `=${value}`;
}

export function parseCellValue(value: string): CellValue {
  if (value === '') return null;
  if (value.startsWith('=')) {
    return {
      type: 'formula',
      formula: normalizeFormula(value),
      cachedValue: null,
    };
  }
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

export function usePendingCellSave({
  fileData,
  currentSheet,
  currentSheetIndex,
  selectedCell,
  cellEditorValue,
  applyMutationResponse,
  markPendingContentChange,
  clearPendingContentChange,
}: UsePendingCellSaveOptions) {
  const documentSessionStore = useDocumentSessionStore();
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  const pendingChanges = documentSessionStore.pendingCellChanges;
  const inFlightChanges = documentSessionStore.inFlightCellChanges;
  const draftCellValues = documentSessionStore.draftCellValues;
  let pendingSavePromise: Promise<boolean> | null = null;
  let needsSaveAfterInFlight = false;

  const currentCellValue = computed(() => {
    if (!selectedCell.value || !currentSheet.value) return undefined;
    return currentSheet.value.rows[selectedCell.value.row]?.[selectedCell.value.col];
  });

  watch(selectedCell, (newCell) => {
    if (newCell && currentSheet.value) {
      cellEditorValue.value = editorStringForCell(currentSheetIndex.value, newCell.row, newCell.col);
    } else {
      cellEditorValue.value = '';
    }
  }, { immediate: true });

  watch(currentCellValue, () => {
    const key = selectedCellKey();
    if (selectedCell.value && (!key || !draftCellValues.has(key))) {
      cellEditorValue.value = editorStringForCell(
        currentSheetIndex.value,
        selectedCell.value.row,
        selectedCell.value.col
      );
    }
  });

  watch(cellEditorValue, (newValue) => {
    if (!selectedCell.value || !currentSheet.value) return;

    const { row, col } = selectedCell.value;
    updateDraftCell(currentSheetIndex.value, row, col, newValue);
  });

  function selectedCellKey() {
    if (!selectedCell.value) return null;
    return getCellKey(currentSheetIndex.value, selectedCell.value.row, selectedCell.value.col);
  }

  function committedCellValue(sheetIndex: number, row: number, col: number): CellValue {
    return fileData.value?.sheets[sheetIndex]?.rows[row]?.[col] ?? null;
  }

  function pendingBaseCellValue(sheetIndex: number, row: number, col: number): CellValue {
    const key = getCellKey(sheetIndex, row, col);
    const inFlightChange = inFlightChanges.get(key);
    return inFlightChange ? parseCellValue(inFlightChange.value) : committedCellValue(sheetIndex, row, col);
  }

  function pendingBaseEditorString(sheetIndex: number, row: number, col: number): string {
    const key = getCellKey(sheetIndex, row, col);
    return inFlightChanges.get(key)?.value ?? cellToEditorString(committedCellValue(sheetIndex, row, col));
  }

  function editorStringForCell(sheetIndex: number, row: number, col: number): string {
    const key = getCellKey(sheetIndex, row, col);
    return draftCellValues.get(key) ?? pendingBaseEditorString(sheetIndex, row, col);
  }

  function updateDraftCell(sheetIndex: number, row: number, col: number, value: string) {
    const originalValue = pendingBaseCellValue(sheetIndex, row, col);
    const key = getCellKey(sheetIndex, row, col);

    if (draftCellValues.get(key) === value) {
      return;
    }

    if (value === cellToEditorString(originalValue)) {
      draftCellValues.delete(key);
      pendingChanges.delete(key);
      clearPendingContentChangeIfIdle();
      return;
    }

    draftCellValues.set(key, value);
    queueCellChange(sheetIndex, row, col, value, originalValue);
    markPendingContentChange();
    schedulePendingSave();
  }

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
      startPendingSave();
    }, 500);
  }

  function startPendingSave() {
    if (pendingSavePromise) {
      needsSaveAfterInFlight = true;
      return;
    }

    pendingSavePromise = debouncedSave().finally(() => {
      pendingSavePromise = null;
      if (needsSaveAfterInFlight || (pendingChanges.size && !debounceTimer)) {
        needsSaveAfterInFlight = false;
        startPendingSave();
        return;
      }
      if (!pendingChanges.size) {
        clearPendingContentChange();
      }
    });
  }

  async function commitCellChange(
    sheetIndex: number,
    rowIndex: number,
    colIndex: number,
    value: string
  ) {
    const currentFileData = fileData.value;
    if (!currentFileData) return;

    const sheet = currentFileData.sheets[sheetIndex];
    if (!sheet) return;

    const newValue = parseCellValue(value);
    const isCurrentCell = currentSheetIndex.value === sheetIndex
      && selectedCell.value?.row === rowIndex
      && selectedCell.value?.col === colIndex;

    const response = await api.setCell(sheetIndex, rowIndex, colIndex, newValue);
    applyMutationResponse(response);

    const key = getCellKey(sheetIndex, rowIndex, colIndex);
    inFlightChanges.delete(key);
    if (draftCellValues.get(key) === value) {
      draftCellValues.delete(key);
    }

    if (isCurrentCell) {
      cellEditorValue.value = editorStringForCell(sheetIndex, rowIndex, colIndex);
    }

  }

  async function debouncedSave(): Promise<boolean> {
    if (!pendingChanges.size) {
      clearPendingContentChange();
      return true;
    }

    const changes = Array.from(pendingChanges.values());
    pendingChanges.clear();
    for (const change of changes) {
      inFlightChanges.set(getCellKey(change.sheetIndex, change.row, change.col), change);
    }

    for (let i = 0; i < changes.length; i += 1) {
      const { sheetIndex, row, col, value } = changes[i];
      try {
        await commitCellChange(sheetIndex, row, col, value);
      } catch (error) {
        for (const change of changes.slice(i)) {
          inFlightChanges.delete(getCellKey(change.sheetIndex, change.row, change.col));
        }
        for (const change of changes.slice(i)) {
          clearDraftIfUnchanged(change);
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

  function clearDraftIfUnchanged(change: PendingCellChange) {
    const key = getCellKey(change.sheetIndex, change.row, change.col);
    if (draftCellValues.get(key) === change.value) {
      draftCellValues.delete(key);
    }
    if (pendingChanges.get(key)?.value === change.value) {
      pendingChanges.delete(key);
    }
  }

  function clearPendingContentChangeIfIdle() {
    if (!pendingChanges.size && !pendingSavePromise) {
      clearPendingContentChange();
    }
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
        startPendingSave();
        if (!pendingSavePromise) return false;
        const saved = await pendingSavePromise;
        if (!saved) return false;
      }

      if (debounceTimer) {
        clearTimeout(debounceTimer);
        debounceTimer = null;
      }
      if (pendingChanges.size) {
        continue;
      }
      if (!pendingSavePromise) {
        return true;
      }
    }
  }

  async function handleCellChange(rowIndex: number, colIndex: number, value: string) {
    if (!currentSheet.value) return;

    updateDraftCell(currentSheetIndex.value, rowIndex, colIndex, value);
    void flushPendingCellChanges();
  }

  function handleCellEditing(row: number, col: number, value: string) {
    if (selectedCell.value?.row === row && selectedCell.value?.col === col) {
      cellEditorValue.value = value;
    }

    if (!currentSheet.value) return;
    updateDraftCell(currentSheetIndex.value, row, col, value);
  }

  function handleCellEditCancel(row: number, col: number) {
    if (!currentSheet.value) return;

    const sheetIndex = currentSheetIndex.value;
    const key = getCellKey(sheetIndex, row, col);
    const inFlightChange = inFlightChanges.get(key);
    draftCellValues.delete(key);
    pendingChanges.delete(key);

    if (inFlightChange) {
      const revertValue = cellToEditorString(inFlightChange.oldValue);
      draftCellValues.set(key, revertValue);
      queueCellChange(sheetIndex, row, col, revertValue, pendingBaseCellValue(sheetIndex, row, col));
      markPendingContentChange();
      schedulePendingSave();
    }

    if (selectedCell.value?.row === row && selectedCell.value?.col === col) {
      cellEditorValue.value = editorStringForCell(sheetIndex, row, col);
    }

    if (!pendingChanges.size && debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }
    clearPendingContentChangeIfIdle();
  }

  function handleCellEditorSubmit() {
    if (!selectedCell.value || !currentSheet.value) return;

    const { row, col } = selectedCell.value;
    updateDraftCell(currentSheetIndex.value, row, col, cellEditorValue.value);
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
    draftCellValues,
    flushPendingCellChanges,
    handleCellChange,
    handleCellEditing,
    handleCellEditCancel,
    handleCellEditorSubmit,
    handleDeselectCell,
  };
}
