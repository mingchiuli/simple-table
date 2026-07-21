import { computed, onUnmounted } from 'vue';
import { ElMessage } from 'element-plus';
import type { ComputedRef } from 'vue';
import {
  usePendingCellSavesStore,
  PendingCellSaveLimitError,
  type CellSaveRequest,
  type PendingCellSaveCallbacks,
  type QueueDraftResult,
} from '@/stores/pendingCellSaves';
import { useDocumentStatusStore } from '@/stores/documentStatus';
import { usePendingCellSaveCoordinator } from '@/composables/usePendingCellSaveCoordinator';
import type { CellValue, DocumentProjection } from '@/types/documentRuntime';
import { blankCell, cellToEditorString } from '@/utils/cellValue';
import { getCellKey } from '@/utils/cellKey';
import { sheetCell } from '@/projection/documentProjection';

type UseCellEditTransactionsOptions = {
  fileData: ComputedRef<DocumentProjection | null>;
  commitBatch: (changes: CellSaveRequest[]) => Promise<void>;
  onBatchCommitted?: () => void;
  onCommitFailed?: (error: unknown) => Promise<void> | void;
  debounceMs?: number;
};

export function useCellEditTransactions({
  fileData,
  commitBatch,
  onBatchCommitted,
  onCommitFailed,
  debounceMs = 500,
}: UseCellEditTransactionsOptions) {
  const pendingCellSavesStore = usePendingCellSavesStore();
  const pendingCellSaveCoordinator = usePendingCellSaveCoordinator();
  const documentStatusStore = useDocumentStatusStore();
  const draftCellValues = computed<Readonly<Record<string, string>>>(
    () => {
      void pendingCellSavesStore.draftVersion;
      return { ...pendingCellSavesStore.draftCellValues };
    }
  );
  const schedulerCallbacks: PendingCellSaveCallbacks = {
    commitBatch,
    clearPendingContentChange: () => documentStatusStore.clearPendingContentChange(),
    onBatchCommitted,
    onCommitFailed,
  };

  function cellKey(sheetIndex: number, row: number, col: number) {
    return getCellKey(sheetIndex, row, col);
  }

  function saveState(sheetIndex: number, row: number, col: number) {
    return pendingCellSavesStore.stateFor(cellKey(sheetIndex, row, col));
  }

  function hasPendingWork() {
    return pendingCellSaveCoordinator.hasPendingWork();
  }

  function committedCellValue(sheetIndex: number, row: number, col: number): CellValue {
    return sheetCell(fileData.value?.sheets[sheetIndex], row, col) ?? blankCell();
  }

  function visibleBaseEditorString(sheetIndex: number, row: number, col: number): string {
    const { active } = saveState(sheetIndex, row, col);
    return active?.value ?? cellToEditorString(committedCellValue(sheetIndex, row, col));
  }

  function editorStringForCell(sheetIndex: number, row: number, col: number): string {
    const { draft } = saveState(sheetIndex, row, col);
    return draft ?? visibleBaseEditorString(sheetIndex, row, col);
  }

  function updateDraftCell(sheetIndex: number, row: number, col: number, value: string): boolean {
    const { key } = saveState(sheetIndex, row, col);
    const committedValue = committedCellValue(sheetIndex, row, col);
    let result: QueueDraftResult;
    try {
      result = pendingCellSavesStore.applyDraft(
        key,
        {
          sheetIndex,
          row,
          col,
          value,
          oldValue: committedValue,
        },
        committedValue
      );
    } catch (error) {
      if (!(error instanceof PendingCellSaveLimitError)) throw error;
      ElMessage.error(error.message);
      return false;
    }

    handleQueueResult(result);
    return true;
  }

  function cancelDraftCell(sheetIndex: number, row: number, col: number) {
    const { key, active } = saveState(sheetIndex, row, col);
    const result = pendingCellSavesStore.cancelDraft(
      key,
      active
        ? {
            sheetIndex,
            row,
            col,
            value: cellToEditorString(active.oldValue),
            oldValue: committedCellValue(sheetIndex, row, col),
          }
        : undefined
    );

    handleQueueResult(result);
  }

  function handleQueueResult(result: QueueDraftResult) {
    if (result.shouldMarkPending) {
      documentStatusStore.markPendingContentChange();
    }
    if (result.queued) {
      schedulePendingSave();
      return;
    }
    clearDebounceIfNoQueuedSaves();
    if (result.shouldClearPendingIfIdle) {
      clearPendingContentChangeIfIdle();
    }
  }

  function schedulePendingSave() {
    pendingCellSaveCoordinator.schedulePendingSave(schedulerCallbacks, debounceMs);
  }

  function clearDebounceIfNoQueuedSaves() {
    pendingCellSaveCoordinator.clearDebounceIfNoQueuedSaves();
  }

  function clearPendingContentChangeIfIdle() {
    pendingCellSaveCoordinator.clearPendingContentChangeIfIdle(() =>
      documentStatusStore.clearPendingContentChange()
    );
  }

  async function flushPendingCellChanges(): Promise<boolean> {
    return pendingCellSaveCoordinator.flushPendingCellChanges(schedulerCallbacks);
  }

  onUnmounted(() => {
    pendingCellSaveCoordinator.releaseSchedulerIfIdle();
  });

  return {
    draftCellValues,
    saveState,
    hasPendingWork,
    committedCellValue,
    visibleBaseEditorString,
    editorStringForCell,
    updateDraftCell,
    cancelDraftCell,
    flushPendingCellChanges,
  };
}
