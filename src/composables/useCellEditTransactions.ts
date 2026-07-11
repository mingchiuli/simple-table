import { computed, onUnmounted } from 'vue';
import type { ComputedRef } from 'vue';
import {
  usePendingCellSavesStore,
  type CellSaveRequest,
  type PendingCellSaveCallbacks,
  type QueueDraftResult,
} from '@/stores/pendingCellSaves';
import { useDocumentStatusStore } from '@/stores/documentStatus';
import type { CellValue, DocumentProjection } from '@/types';
import { blankCell, cellToEditorString } from '@/utils/cellValue';
import { getCellKey } from '@/utils/cellKey';

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
  const documentStatusStore = useDocumentStatusStore();
  const draftCellValues = computed<ReadonlyMap<string, string>>(
    () => pendingCellSavesStore.draftCellValues
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
    return pendingCellSavesStore.hasPendingWork();
  }

  function committedCellValue(sheetIndex: number, row: number, col: number): CellValue {
    const slot = fileData.value?.sheets[sheetIndex];
    return slot?.state === 'loaded' ? slot.data.rows[row]?.[col] ?? blankCell() : blankCell();
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
    const { key } = saveState(sheetIndex, row, col);
    const committedValue = committedCellValue(sheetIndex, row, col);
    const result = pendingCellSavesStore.applyDraft(
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

    handleQueueResult(result);
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
    pendingCellSavesStore.schedulePendingSave(schedulerCallbacks, debounceMs);
  }

  function clearDebounceIfNoQueuedSaves() {
    pendingCellSavesStore.clearDebounceIfNoQueuedSaves();
  }

  function clearPendingContentChangeIfIdle() {
    pendingCellSavesStore.clearPendingContentChangeIfIdle(() =>
      documentStatusStore.clearPendingContentChange()
    );
  }

  async function flushPendingCellChanges(): Promise<boolean> {
    return pendingCellSavesStore.flushPendingCellChanges(schedulerCallbacks);
  }

  onUnmounted(() => {
    pendingCellSavesStore.releaseSchedulerIfIdle();
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
