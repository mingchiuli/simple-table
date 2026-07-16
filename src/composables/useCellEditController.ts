import { computed, watch } from 'vue';
import { ElMessage } from 'element-plus';
import * as api from '@/api';
import { useCellEditTransactions } from '@/composables/useCellEditTransactions';
import { useDocumentCommandBus } from '@/composables/useDocumentCommandBus';
import {
  useDocumentSessionStore,
} from '@/stores/documentSession';
import { useEditorSelectionStore } from '@/stores/editorSelection';
import type { CellSaveRequest } from '@/stores/pendingCellSaves';
import type { ComputedRef, Ref } from 'vue';
import type { DocumentProjection, LoadedSheetSlot, SetCellRequest } from '@/types';
import { isCellLoaded, sheetCell } from '@/stores/documentProjection';
import { cellToEditorString } from '@/utils/cellValue';
import { getCellKey } from '@/utils/cellKey';
import { appErrorMessage } from '@/utils/appError';

type CellPosition = { row: number; col: number };

type UseCellEditControllerOptions = {
  fileData: ComputedRef<DocumentProjection | null>;
  currentSheet: ComputedRef<LoadedSheetSlot | null>;
  currentSheetIndex: Ref<number>;
  selectedCell: Ref<CellPosition | null>;
  cellEditorValue: Ref<string>;
  canEditCells: ComputedRef<boolean>;
};

export function useCellEditController({
  fileData,
  currentSheet,
  currentSheetIndex,
  selectedCell,
  cellEditorValue,
  canEditCells,
}: UseCellEditControllerOptions) {
  const documentSessionStore = useDocumentSessionStore();
  const editorSelectionStore = useEditorSelectionStore();
  const commandBus = useDocumentCommandBus();

  const transactions = useCellEditTransactions({
    fileData,
    commitBatch,
    onBatchCommitted: refreshSelectedEditorValue,
    onCommitFailed: handleCommitFailed,
  });

  const currentCellValue = computed(() => {
    if (!selectedCell.value || !currentSheet.value) return undefined;
    return sheetCell(currentSheet.value, selectedCell.value.row, selectedCell.value.col);
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
    if (selectedCell.value && (!key || !transactions.draftCellValues.value.has(key))) {
      refreshSelectedEditorValue();
    }
  });

  async function commitBatch(changes: CellSaveRequest[]) {
    const context = documentSessionStore.requireCommandContext();
    const documentId = context.documentId;
    const currentFileData = fileData.value;
    if (!currentFileData) throw new Error('No file is loaded');

    const payload: SetCellRequest[] = changes.map((change) => {
      const sheet = currentFileData.sheets[change.sheetIndex];
      if (!sheet || !isCellLoaded(sheet, change.row, change.col)) {
        throw new Error(`Cell ${change.row},${change.col} is not loaded`);
      }
      return {
        sheetIndex: change.sheetIndex,
        row: change.row,
        col: change.col,
        text: change.value,
      };
    });

    await commandBus.runBackgroundMutation({
      documentId,
      action: (context) => api.setCells(context, payload),
      onRefreshFailed: (error) => {
        ElMessage.error(`保存已提交，但刷新失败: ${appErrorMessage(error)}`);
      },
    });
  }

  async function handleCommitFailed(error: unknown) {
    await commandBus.refreshAfterMutationError(true);
    ElMessage.error(`保存失败: ${appErrorMessage(error)}，已恢复所有更改`);
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
    editorSelectionStore.setEditorValue(value);
  }

  async function handleCellChange(rowIndex: number, colIndex: number, value: string) {
    if (!canEditCells.value || !isCellLoaded(currentSheet.value, rowIndex, colIndex)) return;

    if (!transactions.updateDraftCell(currentSheetIndex.value, rowIndex, colIndex, value)) {
      refreshSelectedEditorValue();
      return;
    }
    void transactions.flushPendingCellChanges();
  }

  function handleCellEditing(row: number, col: number, value: string) {
    if (!canEditCells.value) return;
    if (selectedCell.value?.row === row && selectedCell.value?.col === col) {
      syncFormulaBarValue(value);
    }

    if (!isCellLoaded(currentSheet.value, row, col)) return;
    if (!transactions.updateDraftCell(currentSheetIndex.value, row, col, value)) {
      refreshSelectedEditorValue();
    }
  }

  function handleCellEditCancel(row: number, col: number) {
    if (!canEditCells.value || !isCellLoaded(currentSheet.value, row, col)) return;

    transactions.cancelDraftCell(currentSheetIndex.value, row, col);
    if (selectedCell.value?.row === row && selectedCell.value?.col === col) {
      refreshSelectedEditorValue();
    }
  }

  function handleCellEditorSubmit() {
    if (!canEditCells.value || !selectedCell.value || !currentSheet.value) return;

    const { row, col } = selectedCell.value;
    if (!isCellLoaded(currentSheet.value, row, col)) return;
    transactions.updateDraftCell(currentSheetIndex.value, row, col, cellEditorValue.value);
    void transactions.flushPendingCellChanges();
  }

  function handleDeselectCell() {
    editorSelectionStore.clearSelection();
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
