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
  canEditCells: ComputedRef<boolean>;
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

export function usePendingCellSave({
  fileData,
  currentSheet,
  currentSheetIndex,
  selectedCell,
  cellEditorValue,
  canEditCells,
  applyMutationResponse,
  markPendingContentChange,
  clearPendingContentChange,
}: UsePendingCellSaveOptions) {
  const pendingCellSavesStore = usePendingCellSavesStore();
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  const draftCellValues = pendingCellSavesStore.draftCellValues;
  let pendingSavePromise: Promise<boolean> | null = null;

  function cellKey(sheetIndex: number, row: number, col: number) {
    return getCellKey(sheetIndex, row, col);
  }

  function saveState(sheetIndex: number, row: number, col: number) {
    return pendingCellSavesStore.stateFor(cellKey(sheetIndex, row, col));
  }

  function hasPendingWork() {
    return !pendingCellSavesStore.isIdle() || pendingSavePromise !== null;
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
    if (!canEditCells.value || !selectedCell.value || !currentSheet.value) return;

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

    if (activeSave && value === activeSave.value) {
      pendingCellSavesStore.setDraft(key, value);
      pendingCellSavesStore.dropQueued(key);
      clearPendingContentChangeIfIdle();
      return;
    }

    if (activeSave && value === cellToEditorString(activeSave.oldValue)) {
      pendingCellSavesStore.setDraft(key, value);
      queueCellSave(sheetIndex, row, col, value, committedValue);
      markPendingContentChange();
      schedulePendingSave();
      return;
    }

    if (!activeSave && value === cellToEditorString(committedValue)) {
      pendingCellSavesStore.clearDraft(key);
      pendingCellSavesStore.dropQueued(key);
      clearPendingContentChangeIfIdle();
      return;
    }

    if (draftCellValues.get(key) === value && queued?.value === value) {
      return;
    }

    pendingCellSavesStore.setDraft(key, value);
    queueCellSave(sheetIndex, row, col, value, committedValue);
    markPendingContentChange();
    schedulePendingSave();
  }

  function queueCellSave(sheetIndex: number, row: number, col: number, value: string, oldValue: CellValue) {
    const key = cellKey(sheetIndex, row, col);
    pendingCellSavesStore.queueSave(key, {
      sheetIndex,
      row,
      col,
      value,
      oldValue,
    });
  }

  function schedulePendingSave() {
    if (debounceTimer) {
      clearTimeout(debounceTimer);
    }
    pendingCellSavesStore.setPhase('debouncing');
    debounceTimer = setTimeout(() => {
      debounceTimer = null;
      startPendingSave();
    }, 500);
  }

  function startPendingSave() {
    if (pendingSavePromise) {
      return;
    }

    pendingCellSavesStore.setPhase('saving');
    pendingSavePromise = debouncedSave().finally(() => {
      pendingSavePromise = null;
      if (pendingCellSavesStore.hasQueuedSaves && !debounceTimer) {
        startPendingSave();
        return;
      }
      if (pendingCellSavesStore.isIdle()) {
        pendingCellSavesStore.setPhase('idle');
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
        text: change.value,
      };
    });

    const selectedKey = selectedCell.value
      ? cellKey(currentSheetIndex.value, selectedCell.value.row, selectedCell.value.col)
      : null;
    const response = await api.setCells(payload);
    applyMutationResponse(response);

    pendingCellSavesStore.completeBatch(changes);

    if (selectedCell.value && selectedKey) {
      cellEditorValue.value = editorStringForCell(
        currentSheetIndex.value,
        selectedCell.value.row,
        selectedCell.value.col
      );
    }
  }

  async function debouncedSave(): Promise<boolean> {
    if (!pendingCellSavesStore.hasQueuedSaves) {
      clearPendingContentChange();
      return true;
    }

    const changes = pendingCellSavesStore.takeQueuedBatch();

    try {
      await commitCellBatch(changes);
    } catch (error) {
      pendingCellSavesStore.failBatch(changes);
      pendingCellSavesStore.setPhase('failed', String(error));
      ElMessage.error(`保存失败: ${error}，已恢复所有更改`);
      if (pendingCellSavesStore.isIdle()) {
        clearPendingContentChange();
      }
      return false;
    }
    if (pendingCellSavesStore.isIdle()) {
      pendingCellSavesStore.setPhase('idle');
      clearPendingContentChange();
    }
    return true;
  }

  function clearPendingContentChangeIfIdle() {
    if (!hasPendingWork()) {
      pendingCellSavesStore.setPhase('idle');
      clearPendingContentChange();
    }
  }

  async function flushPendingCellChanges(): Promise<boolean> {
    if (debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
      if (pendingCellSavesStore.hasQueuedSaves) {
        pendingCellSavesStore.setPhase('saving');
      }
    }

    while (true) {
      if (pendingSavePromise) {
        const saved = await pendingSavePromise;
        if (!saved) return false;
      } else if (pendingCellSavesStore.hasQueuedSaves) {
        startPendingSave();
        if (!pendingSavePromise) return false;
        const saved = await pendingSavePromise;
        if (!saved) return false;
      }

      if (debounceTimer) {
        clearTimeout(debounceTimer);
        debounceTimer = null;
      }
      if (pendingCellSavesStore.hasQueuedSaves) {
        continue;
      }
      if (!pendingSavePromise) {
        return true;
      }
    }
  }

  async function handleCellChange(rowIndex: number, colIndex: number, value: string) {
    if (!canEditCells.value || !currentSheet.value) return;

    updateDraftCell(currentSheetIndex.value, rowIndex, colIndex, value);
    void flushPendingCellChanges();
  }

  function handleCellEditing(row: number, col: number, value: string) {
    if (!canEditCells.value) return;
    if (selectedCell.value?.row === row && selectedCell.value?.col === col) {
      cellEditorValue.value = value;
    }

    if (!currentSheet.value) return;
    updateDraftCell(currentSheetIndex.value, row, col, value);
  }

  function handleCellEditCancel(row: number, col: number) {
    if (!canEditCells.value || !currentSheet.value) return;

    const sheetIndex = currentSheetIndex.value;
    const { key, active: activeSave } = saveState(sheetIndex, row, col);
    pendingCellSavesStore.clearDraft(key);
    pendingCellSavesStore.dropQueued(key);

    if (activeSave) {
      const revertValue = cellToEditorString(activeSave.oldValue);
      pendingCellSavesStore.setDraft(key, revertValue);
      queueCellSave(sheetIndex, row, col, revertValue, committedCellValue(sheetIndex, row, col));
      markPendingContentChange();
      schedulePendingSave();
    }

    if (selectedCell.value?.row === row && selectedCell.value?.col === col) {
      cellEditorValue.value = editorStringForCell(sheetIndex, row, col);
    }

    if (!pendingCellSavesStore.hasQueuedSaves && debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
      if (!pendingCellSavesStore.hasActiveSaves) {
        pendingCellSavesStore.setPhase('idle');
      }
    }
    clearPendingContentChangeIfIdle();
  }

  function handleCellEditorSubmit() {
    if (!canEditCells.value || !selectedCell.value || !currentSheet.value) return;

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
    if (!pendingSavePromise && pendingCellSavesStore.isIdle()) {
      pendingCellSavesStore.setPhase('idle');
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
