import { computed, watch } from 'vue';
import { ElMessage } from 'element-plus';
import * as api from '@/api';
import { useCellEditTransactions } from '@/composables/useCellEditTransactions';
import { enqueueEditorMutation } from '@/composables/useEditorMutationQueue';
import { useDocumentSessionStore } from '@/stores/documentSession';
import type { CellSaveRequest } from '@/stores/pendingCellSaves';
import type { ComputedRef, Ref } from 'vue';
import type { EditorMutationResponse, FileData, SetCellRequest, SheetData } from '@/types';
import { cellToEditorString } from '@/utils/cellValue';
import { getCellKey } from '@/utils/cellKey';

type CellPosition = { row: number; col: number };

type UseCellEditControllerOptions = {
  fileData: ComputedRef<FileData | null>;
  currentSheet: ComputedRef<SheetData | null>;
  currentSheetIndex: Ref<number>;
  selectedCell: Ref<CellPosition | null>;
  cellEditorValue: Ref<string>;
  canEditCells: ComputedRef<boolean>;
  applyMutationResponse: (response: EditorMutationResponse) => Promise<void>;
  markPendingContentChange: () => void;
  clearPendingContentChange: () => void;
};

export function useCellEditController({
  fileData,
  currentSheet,
  currentSheetIndex,
  selectedCell,
  cellEditorValue,
  canEditCells,
  applyMutationResponse,
  markPendingContentChange,
  clearPendingContentChange,
}: UseCellEditControllerOptions) {
  const documentSessionStore = useDocumentSessionStore();
  let syncingEditorValue = false;

  const transactions = useCellEditTransactions({
    fileData,
    commitBatch,
    markPendingContentChange,
    clearPendingContentChange,
    onBatchCommitted: refreshSelectedEditorValue,
    onCommitFailed: handleCommitFailed,
  });

  const currentCellValue = computed(() => {
    if (!selectedCell.value || !currentSheet.value) return undefined;
    return currentSheet.value.rows[selectedCell.value.row]?.[selectedCell.value.col];
  });

  watch(
    selectedCell,
    (newCell) => {
      if (newCell && currentSheet.value) {
        syncFormulaBarValue(
          transactions.editorStringForCell(currentSheetIndex.value, newCell.row, newCell.col)
        );
      } else {
        syncFormulaBarValue('');
      }
    },
    { immediate: true }
  );

  watch(currentCellValue, () => {
    const key = selectedCellKey();
    if (selectedCell.value && (!key || !transactions.draftCellValues.has(key))) {
      refreshSelectedEditorValue();
    }
  });

  watch(cellEditorValue, (newValue) => {
    if (syncingEditorValue) return;
    if (!canEditCells.value || !selectedCell.value || !currentSheet.value) return;

    const { row, col } = selectedCell.value;
    transactions.updateDraftCell(currentSheetIndex.value, row, col, newValue);
  }, { flush: 'sync' });

  async function commitBatch(changes: CellSaveRequest[]) {
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

    await enqueueEditorMutation(documentSessionStore.mutationScope, async () => {
      const response = await api.setCells(payload);
      await applyMutationResponse(response);
    });
  }

  async function handleCommitFailed(error: unknown) {
    await refreshSessionAfterMutationError();
    ElMessage.error(`保存失败: ${error}，已恢复所有更改`);
  }

  async function refreshSessionAfterMutationError() {
    try {
      documentSessionStore.applyEditorSession(await api.getEditorState());
    } catch (error) {
      console.error('Failed to refresh editor state after cell save error:', error);
    }
  }

  function selectedCellKey() {
    if (!selectedCell.value) return null;
    return getCellKey(currentSheetIndex.value, selectedCell.value.row, selectedCell.value.col);
  }

  function refreshSelectedEditorValue() {
    if (!selectedCell.value) return;
    syncFormulaBarValue(
      transactions.editorStringForCell(
        currentSheetIndex.value,
        selectedCell.value.row,
        selectedCell.value.col
      )
    );
  }

  function syncFormulaBarValue(value: string) {
    syncingEditorValue = true;
    cellEditorValue.value = value;
    syncingEditorValue = false;
  }

  async function handleCellChange(rowIndex: number, colIndex: number, value: string) {
    if (!canEditCells.value || !currentSheet.value) return;

    transactions.updateDraftCell(currentSheetIndex.value, rowIndex, colIndex, value);
    void transactions.flushPendingCellChanges();
  }

  function handleCellEditing(row: number, col: number, value: string) {
    if (!canEditCells.value) return;
    if (selectedCell.value?.row === row && selectedCell.value?.col === col) {
      syncFormulaBarValue(value);
    }

    if (!currentSheet.value) return;
    transactions.updateDraftCell(currentSheetIndex.value, row, col, value);
  }

  function handleCellEditCancel(row: number, col: number) {
    if (!canEditCells.value || !currentSheet.value) return;

    transactions.cancelDraftCell(currentSheetIndex.value, row, col);
    if (selectedCell.value?.row === row && selectedCell.value?.col === col) {
      refreshSelectedEditorValue();
    }
  }

  function handleCellEditorSubmit() {
    if (!canEditCells.value || !selectedCell.value || !currentSheet.value) return;

    const { row, col } = selectedCell.value;
    transactions.updateDraftCell(currentSheetIndex.value, row, col, cellEditorValue.value);
    void transactions.flushPendingCellChanges();
  }

  function handleDeselectCell() {
    selectedCell.value = null;
    syncFormulaBarValue('');
  }

  return {
    cellToEditorString,
    draftCellValues: transactions.draftCellValues,
    flushPendingCellChanges: transactions.flushPendingCellChanges,
    refreshSelectedEditorValue,
    handleCellChange,
    handleCellEditing,
    handleCellEditCancel,
    handleCellEditorSubmit,
    handleDeselectCell,
  };
}
