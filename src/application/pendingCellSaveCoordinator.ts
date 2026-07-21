import type {
  CellSaveRequest,
  PendingCellSaveCallbacks,
  PendingCellSavePhase,
} from '@/types/pendingCellSave';

export type PendingCellSavePort = {
  readonly hasQueuedSaves: boolean;
  readonly hasActiveSaves: boolean;
  readonly phase: PendingCellSavePhase;
  setPhase(phase: PendingCellSavePhase, error?: string | null): void;
  takeQueuedBatch(): CellSaveRequest[];
  completeBatch(batch: CellSaveRequest[]): void;
  failBatch(batch: CellSaveRequest[]): void;
  isIdle(): boolean;
  reset(): void;
};

export function createPendingCellSaveCoordinator(store: PendingCellSavePort) {
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  let pendingSavePromise: Promise<boolean> | null = null;
  let pendingSaveGeneration: number | null = null;
  let generation = 0;
  let autosaveSuspendCount = 0;
  let lastCallbacks: PendingCellSaveCallbacks | null = null;
  let lastDebounceMs = 0;

  function hasPendingWork() {
    return !store.isIdle() || pendingSavePromise !== null || debounceTimer !== null;
  }

  function schedulePendingSave(callbacks: PendingCellSaveCallbacks, debounceMs: number) {
    lastCallbacks = callbacks;
    lastDebounceMs = debounceMs;
    clearDebounceTimer();
    store.setPhase('debouncing');
    if (autosaveSuspendCount > 0) return;
    const scheduledGeneration = generation;
    debounceTimer = setTimeout(() => {
      if (scheduledGeneration !== generation) return;
      debounceTimer = null;
      startPendingSave(callbacks);
    }, debounceMs);
  }

  function startPendingSave(callbacks: PendingCellSaveCallbacks) {
    lastCallbacks = callbacks;
    if (pendingSavePromise || autosaveSuspendCount > 0) return;

    store.setPhase('saving');
    const saveGeneration = generation;
    const savePromise = saveQueuedBatch(callbacks, saveGeneration);
    pendingSavePromise = savePromise;
    pendingSaveGeneration = saveGeneration;
    void savePromise.finally(() => {
      if (pendingSavePromise === savePromise) {
        pendingSavePromise = null;
        pendingSaveGeneration = null;
      }
      if (saveGeneration !== generation) {
        resumeCurrentGenerationSaveIfNeeded();
        return;
      }
      if (store.hasQueuedSaves && !debounceTimer) {
        startPendingSave(callbacks);
        return;
      }
      if (store.isIdle() && store.phase !== 'failed') {
        store.setPhase('idle');
        safeClearPendingContentChange(callbacks);
      }
    });
  }

  async function saveQueuedBatch(
    callbacks: PendingCellSaveCallbacks,
    saveGeneration: number,
  ): Promise<boolean> {
    if (!store.hasQueuedSaves) {
      safeClearPendingContentChange(callbacks);
      return true;
    }

    const changes = store.takeQueuedBatch();
    try {
      await callbacks.commitBatch(changes);
      if (saveGeneration !== generation) return false;
      store.completeBatch(changes);
      try {
        callbacks.onBatchCommitted?.();
      } catch (error) {
        console.error('Pending cell save committed, but post-commit handling failed:', error);
      }
    } catch (error) {
      if (saveGeneration !== generation) return false;
      let failureReason = error;
      try {
        await callbacks.onCommitFailed?.(error);
      } catch (failureHandlerError) {
        failureReason = failureHandlerError;
        console.error('Pending cell save failure handling failed:', failureHandlerError);
      }
      store.failBatch(changes);
      store.setPhase('failed', String(failureReason));
      if (store.isIdle()) safeClearPendingContentChange(callbacks);
      return false;
    }

    if (store.isIdle()) {
      store.setPhase('idle');
      safeClearPendingContentChange(callbacks);
    }
    return true;
  }

  function clearDebounceIfNoQueuedSaves() {
    if (store.hasQueuedSaves || !debounceTimer) return;
    clearDebounceTimer();
    if (!store.hasActiveSaves) store.setPhase('idle');
  }

  function clearPendingContentChangeIfIdle(clearPendingContentChange: () => void) {
    if (hasPendingWork()) return;
    store.setPhase('idle');
    safeClearPendingContentChangeCallback(clearPendingContentChange);
  }

  function suspendAutosave(): () => void {
    const suspendGeneration = generation;
    let released = false;
    autosaveSuspendCount += 1;
    clearDebounceTimer();

    return () => {
      if (released) return;
      released = true;
      if (suspendGeneration !== generation) return;
      autosaveSuspendCount = Math.max(0, autosaveSuspendCount - 1);
      if (
        autosaveSuspendCount === 0
        && store.hasQueuedSaves
        && !pendingSavePromise
        && !debounceTimer
        && lastCallbacks
      ) {
        schedulePendingSave(lastCallbacks, lastDebounceMs);
        return;
      }
      if (store.isIdle() && !pendingSavePromise && !debounceTimer) store.setPhase('idle');
    };
  }

  async function flushPendingCellChanges(callbacks: PendingCellSaveCallbacks): Promise<boolean> {
    lastCallbacks = callbacks;
    if (debounceTimer) {
      clearDebounceTimer();
      if (store.hasQueuedSaves) store.setPhase('saving');
    }

    while (true) {
      if (pendingSavePromise) {
        const saveGeneration = pendingSaveGeneration;
        const saved = await pendingSavePromise;
        if (!saved && saveGeneration === generation) return false;
      } else if (store.hasQueuedSaves) {
        startPendingSave(callbacks);
        if (!pendingSavePromise) return false;
        const saveGeneration = pendingSaveGeneration;
        const saved = await pendingSavePromise;
        if (!saved && saveGeneration === generation) return false;
      }

      if (debounceTimer) clearDebounceTimer();
      if (store.hasQueuedSaves) continue;
      if (!pendingSavePromise) return true;
    }
  }

  function waitForInFlightSave(): Promise<boolean> {
    return pendingSavePromise ?? Promise.resolve(true);
  }

  function releaseSchedulerIfIdle() {
    if (hasPendingWork()) return;
    clearDebounceTimer();
    store.setPhase('idle');
  }

  function reset() {
    generation += 1;
    clearDebounceTimer();
    autosaveSuspendCount = 0;
    lastCallbacks = null;
    store.reset();
  }

  function resumeCurrentGenerationSaveIfNeeded() {
    if (
      pendingSavePromise
      || autosaveSuspendCount > 0
      || debounceTimer
      || !store.hasQueuedSaves
      || !lastCallbacks
    ) return;
    startPendingSave(lastCallbacks);
  }

  function clearDebounceTimer() {
    if (!debounceTimer) return;
    clearTimeout(debounceTimer);
    debounceTimer = null;
  }

  return {
    hasPendingWork,
    schedulePendingSave,
    startPendingSave,
    clearDebounceIfNoQueuedSaves,
    clearPendingContentChangeIfIdle,
    suspendAutosave,
    flushPendingCellChanges,
    waitForInFlightSave,
    releaseSchedulerIfIdle,
    reset,
  };
}

function safeClearPendingContentChange(callbacks: PendingCellSaveCallbacks) {
  safeClearPendingContentChangeCallback(callbacks.clearPendingContentChange);
}

function safeClearPendingContentChangeCallback(clearPendingContentChange: () => void) {
  try {
    clearPendingContentChange();
  } catch (error) {
    console.error('Pending cell save state was updated, but dirty-state cleanup failed:', error);
  }
}
