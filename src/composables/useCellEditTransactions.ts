import { onUnmounted } from 'vue';
import type { ComputedRef } from 'vue';
import {
  usePendingCellSavesStore,
  type CellSaveRequest,
  type QueueDraftResult,
} from '@/stores/pendingCellSaves';
import type { CellValue, FileData } from '@/types';
import { blankCell, cellToEditorString } from '@/utils/cellValue';
import { getCellKey } from '@/utils/cellKey';

type UseCellEditTransactionsOptions = {
  fileData: ComputedRef<FileData | null>;
  commitBatch: (changes: CellSaveRequest[]) => Promise<void>;
  markPendingContentChange: () => void;
  clearPendingContentChange: () => void;
  onBatchCommitted?: () => void;
  onCommitFailed?: (error: unknown) => Promise<void> | void;
  debounceMs?: number;
};

export function useCellEditTransactions({
  fileData,
  commitBatch,
  markPendingContentChange,
  clearPendingContentChange,
  onBatchCommitted,
  onCommitFailed,
  debounceMs = 500,
}: UseCellEditTransactionsOptions) {
  const pendingCellSavesStore = usePendingCellSavesStore();
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  let pendingSavePromise: Promise<boolean> | null = null;
  const draftCellValues = pendingCellSavesStore.draftCellValues;

  function cellKey(sheetIndex: number, row: number, col: number) {
    return getCellKey(sheetIndex, row, col);
  }

  function saveState(sheetIndex: number, row: number, col: number) {
    return pendingCellSavesStore.stateFor(cellKey(sheetIndex, row, col));
  }

  function hasPendingWork() {
    return !pendingCellSavesStore.isIdle() || pendingSavePromise !== null;
  }

  function committedCellValue(sheetIndex: number, row: number, col: number): CellValue {
    return fileData.value?.sheets[sheetIndex]?.rows[row]?.[col] ?? blankCell();
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
      markPendingContentChange();
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
    if (debounceTimer) {
      clearTimeout(debounceTimer);
    }
    pendingCellSavesStore.setPhase('debouncing');
    debounceTimer = setTimeout(() => {
      debounceTimer = null;
      startPendingSave();
    }, debounceMs);
  }

  function startPendingSave() {
    if (pendingSavePromise) {
      return;
    }

    pendingCellSavesStore.setPhase('saving');
    pendingSavePromise = saveQueuedBatch().finally(() => {
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

  async function saveQueuedBatch(): Promise<boolean> {
    if (!pendingCellSavesStore.hasQueuedSaves) {
      clearPendingContentChange();
      return true;
    }

    const changes = pendingCellSavesStore.takeQueuedBatch();

    try {
      await commitBatch(changes);
      pendingCellSavesStore.completeBatch(changes);
      onBatchCommitted?.();
    } catch (error) {
      await onCommitFailed?.(error);
      pendingCellSavesStore.failBatch(changes);
      pendingCellSavesStore.setPhase('failed', String(error));
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

  function clearDebounceIfNoQueuedSaves() {
    if (pendingCellSavesStore.hasQueuedSaves || !debounceTimer) {
      return;
    }
    clearTimeout(debounceTimer);
    debounceTimer = null;
    if (!pendingCellSavesStore.hasActiveSaves) {
      pendingCellSavesStore.setPhase('idle');
    }
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
