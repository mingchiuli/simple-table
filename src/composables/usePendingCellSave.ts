import type { ComputedRef, Ref } from 'vue';
import * as api from '@/api';
import { usePendingCellSavesStore, type CellSaveRequest } from '@/stores/pendingCellSaves';
import type { CellValue, EditorMutationResponse, FileData, SetCellRequest, SheetData } from '@/types';
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
  const pendingCellSavesStore = usePendingCellSavesStore();
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  const queuedSaves = pendingCellSavesStore.queuedCellSaves;
  const activeSaves = pendingCellSavesStore.activeCellSaves;
  const draftCellValues = pendingCellSavesStore.draftCellValues;
  let pendingSavePromise: Promise<boolean> | null = null;

  function cellKey(sheetIndex: number, row: number, col: number) {
    return getCellKey(sheetIndex, row, col);
  }

  function saveState(sheetIndex: number, row: number, col: number) {
    const key = cellKey(sheetIndex, row, col);
    return {
      key,
      draft: draftCellValues.get(key),
      queued: queuedSaves.get(key),
      active: activeSaves.get(key),
    };
  }

  function hasPendingWork() {
    return queuedSaves.size > 0 || activeSaves.size > 0 || pendingSavePromise !== null;
  }

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
    return cellKey(currentSheetIndex.value, selectedCell.value.row, selectedCell.value.col);
  }

  function committedCellValue(sheetIndex: number, row: number, col: number): CellValue {
    return fileData.value?.sheets[sheetIndex]?.rows[row]?.[col] ?? null;
  }

  function visibleBaseCellValue(sheetIndex: number, row: number, col: number): CellValue {
    const { active } = saveState(sheetIndex, row, col);
    return active ? parseCellValue(active.value) : committedCellValue(sheetIndex, row, col);
  }

  function visibleBaseEditorString(sheetIndex: number, row: number, col: number): string {
    const { active } = saveState(sheetIndex, row, col);
    return active?.value ?? cellToEditorString(committedCellValue(sheetIndex, row, col));
  }

  function editorStringForCell(sheetIndex: number, row: number, col: number): string {
    const { draft } = saveState(sheetIndex, row, col);
    return draft ?? visibleBaseEditorString(sheetIndex, row, col);
  }

  function updateDraftCell(sheetIndex: number, row: number, col: number, value: string) {
    const { key, active: activeSave, queued } = saveState(sheetIndex, row, col);
    const committedValue = committedCellValue(sheetIndex, row, col);
    const visibleBaseValue = activeSave ? parseCellValue(activeSave.value) : committedValue;

    if (activeSave && value === activeSave.value) {
      draftCellValues.set(key, value);
      queuedSaves.delete(key);
      clearPendingContentChangeIfIdle();
      return;
    }

    if (activeSave && value === cellToEditorString(activeSave.oldValue)) {
      draftCellValues.set(key, value);
      queueCellSave(sheetIndex, row, col, value, visibleBaseValue);
      markPendingContentChange();
      schedulePendingSave();
      return;
    }

    if (!activeSave && value === cellToEditorString(committedValue)) {
      draftCellValues.delete(key);
      queuedSaves.delete(key);
      clearPendingContentChangeIfIdle();
      return;
    }

    if (draftCellValues.get(key) === value && queued?.value === value) {
      return;
    }

    draftCellValues.set(key, value);
    queueCellSave(sheetIndex, row, col, value, visibleBaseValue);
    markPendingContentChange();
    schedulePendingSave();
  }

  function queueCellSave(sheetIndex: number, row: number, col: number, value: string, oldValue: CellValue) {
    const { key, queued: existing, active: activeSave } = saveState(sheetIndex, row, col);
    queuedSaves.set(key, {
      sheetIndex,
      row,
      col,
      value,
      oldValue: existing?.oldValue ?? activeSave?.oldValue ?? oldValue,
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
      return;
    }

    pendingSavePromise = debouncedSave().finally(() => {
      pendingSavePromise = null;
      if (queuedSaves.size && !debounceTimer) {
        startPendingSave();
        return;
      }
      if (!queuedSaves.size && !activeSaves.size) {
        clearPendingContentChange();
      }
    });
  }

  async function commitCellBatch(changes: CellSaveRequest[]) {
    const currentFileData = fileData.value;
    if (!currentFileData) throw new Error('No file is loaded');

    const payload: SetCellRequest[] = changes.map((change) => {
      const sheet = currentFileData.sheets[change.sheetIndex];
      if (!sheet) throw new Error(`Sheet ${change.sheetIndex} does not exist`);
      return {
        sheetIndex: change.sheetIndex,
        row: change.row,
        col: change.col,
        newValue: parseCellValue(change.value),
      };
    });

    const selectedKey = selectedCell.value
      ? cellKey(currentSheetIndex.value, selectedCell.value.row, selectedCell.value.col)
      : null;
    const response = await api.setCells(payload);
    applyMutationResponse(response);

    for (const change of changes) {
      const key = cellKey(change.sheetIndex, change.row, change.col);
      activeSaves.delete(key);
      if (draftCellValues.get(key) === change.value) {
        draftCellValues.delete(key);
      }
    }

    if (selectedCell.value && selectedKey) {
      cellEditorValue.value = editorStringForCell(
        currentSheetIndex.value,
        selectedCell.value.row,
        selectedCell.value.col
      );
    }
  }

  async function debouncedSave(): Promise<boolean> {
    if (!queuedSaves.size) {
      clearPendingContentChange();
      return true;
    }

    const changes = Array.from(queuedSaves.values());
    queuedSaves.clear();
    for (const change of changes) {
      activeSaves.set(cellKey(change.sheetIndex, change.row, change.col), change);
    }

    try {
      await commitCellBatch(changes);
    } catch (error) {
      for (const change of changes) {
        activeSaves.delete(cellKey(change.sheetIndex, change.row, change.col));
        clearDraftIfUnchanged(change);
      }
      ElMessage.error(`保存失败: ${error}，已恢复所有更改`);
      if (!queuedSaves.size && !activeSaves.size) {
        clearPendingContentChange();
      }
      return false;
    }
    if (!queuedSaves.size && !activeSaves.size) {
      clearPendingContentChange();
    }
    return true;
  }

  function clearDraftIfUnchanged(change: CellSaveRequest) {
    const key = cellKey(change.sheetIndex, change.row, change.col);
    if (draftCellValues.get(key) === change.value) {
      draftCellValues.delete(key);
    }
    if (queuedSaves.get(key)?.value === change.value) {
      queuedSaves.delete(key);
    }
  }

  function clearPendingContentChangeIfIdle() {
    if (!hasPendingWork()) {
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
      } else if (queuedSaves.size) {
        startPendingSave();
        if (!pendingSavePromise) return false;
        const saved = await pendingSavePromise;
        if (!saved) return false;
      }

      if (debounceTimer) {
        clearTimeout(debounceTimer);
        debounceTimer = null;
      }
      if (queuedSaves.size) {
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
    const { key, active: activeSave } = saveState(sheetIndex, row, col);
    draftCellValues.delete(key);
    queuedSaves.delete(key);

    if (activeSave) {
      const revertValue = cellToEditorString(activeSave.oldValue);
      draftCellValues.set(key, revertValue);
      queueCellSave(sheetIndex, row, col, revertValue, visibleBaseCellValue(sheetIndex, row, col));
      markPendingContentChange();
      schedulePendingSave();
    }

    if (selectedCell.value?.row === row && selectedCell.value?.col === col) {
      cellEditorValue.value = editorStringForCell(sheetIndex, row, col);
    }

    if (!queuedSaves.size && debounceTimer) {
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
