import type { CellValue } from "@/types";
import { cellToEditorString } from "@/utils/cellValue";

export type CellSaveRequest = {
  sheetIndex: number;
  row: number;
  col: number;
  value: string;
  oldValue: CellValue;
};

export type CellSaveState = {
  key: string;
  draft?: string;
  queued?: CellSaveRequest;
  active?: CellSaveRequest;
};

export type PendingCellSavePhase = "idle" | "debouncing" | "saving" | "failed";

export type QueueDraftResult = {
  queued: boolean;
  shouldMarkPending: boolean;
  shouldClearPendingIfIdle: boolean;
};

export type PendingCellSaveCallbacks = {
  commitBatch: (changes: CellSaveRequest[]) => Promise<void>;
  clearPendingContentChange: () => void;
  onBatchCommitted?: () => void;
  onCommitFailed?: (error: unknown) => Promise<void> | void;
};

let debounceTimer: ReturnType<typeof setTimeout> | null = null;
let pendingSavePromise: Promise<boolean> | null = null;
let schedulerGeneration = 0;

export const usePendingCellSavesStore = defineStore("pendingCellSaves", {
  state: () => ({
    draftCellValues: new Map<string, string>(),
    queuedCellSaves: new Map<string, CellSaveRequest>(),
    activeCellSaves: new Map<string, CellSaveRequest>(),
    phase: "idle" as PendingCellSavePhase,
    lastError: null as string | null,
  }),
  getters: {
    hasQueuedSaves: (state) => state.queuedCellSaves.size > 0,
    hasActiveSaves: (state) => state.activeCellSaves.size > 0,
  },
  actions: {
    hasPendingWork() {
      return !this.isIdle() || pendingSavePromise !== null || debounceTimer !== null;
    },
    stateFor(key: string): CellSaveState {
      return {
        key,
        draft: this.draftCellValues.get(key),
        queued: this.queuedCellSaves.get(key),
        active: this.activeCellSaves.get(key),
      };
    },
    setDraft(key: string, value: string) {
      this.draftCellValues.set(key, value);
    },
    clearDraft(key: string) {
      this.draftCellValues.delete(key);
    },
    setPhase(phase: PendingCellSavePhase, error: string | null = null) {
      this.phase = phase;
      this.lastError = error;
    },
    queueSave(key: string, request: CellSaveRequest) {
      const existing = this.queuedCellSaves.get(key);
      const active = this.activeCellSaves.get(key);
      this.queuedCellSaves.set(key, {
        ...request,
        oldValue: existing?.oldValue ?? active?.oldValue ?? request.oldValue,
      });
    },
    applyDraft(
      key: string,
      request: CellSaveRequest,
      committedValue: CellValue
    ): QueueDraftResult {
      const active = this.activeCellSaves.get(key);
      const queued = this.queuedCellSaves.get(key);
      const value = request.value;

      if (active && value === active.value) {
        this.setDraft(key, value);
        this.dropQueued(key);
        return { queued: false, shouldMarkPending: false, shouldClearPendingIfIdle: true };
      }

      if (active && value === cellToEditorString(active.oldValue)) {
        this.setDraft(key, value);
        this.queueSave(key, request);
        return { queued: true, shouldMarkPending: true, shouldClearPendingIfIdle: false };
      }

      if (!active && value === cellToEditorString(committedValue)) {
        this.clearDraft(key);
        this.dropQueued(key);
        return { queued: false, shouldMarkPending: false, shouldClearPendingIfIdle: true };
      }

      if (this.draftCellValues.get(key) === value && queued?.value === value) {
        return { queued: false, shouldMarkPending: false, shouldClearPendingIfIdle: false };
      }

      this.setDraft(key, value);
      this.queueSave(key, request);
      return { queued: true, shouldMarkPending: true, shouldClearPendingIfIdle: false };
    },
    cancelDraft(key: string, requestForRevert?: CellSaveRequest): QueueDraftResult {
      this.clearDraft(key);
      this.dropQueued(key);
      if (!requestForRevert) {
        return { queued: false, shouldMarkPending: false, shouldClearPendingIfIdle: true };
      }
      this.setDraft(key, requestForRevert.value);
      this.queueSave(key, requestForRevert);
      return { queued: true, shouldMarkPending: true, shouldClearPendingIfIdle: false };
    },
    dropQueued(key: string) {
      this.queuedCellSaves.delete(key);
    },
    takeQueuedBatch(): CellSaveRequest[] {
      const batch = Array.from(this.queuedCellSaves.values());
      this.queuedCellSaves.clear();
      for (const request of batch) {
        this.activeCellSaves.set(cellKey(request), request);
      }
      return batch;
    },
    completeBatch(batch: CellSaveRequest[]) {
      for (const request of batch) {
        const key = cellKey(request);
        this.activeCellSaves.delete(key);
        if (this.draftCellValues.get(key) === request.value) {
          this.draftCellValues.delete(key);
        }
      }
    },
    failBatch(batch: CellSaveRequest[]) {
      for (const request of batch) {
        const key = cellKey(request);
        this.activeCellSaves.delete(key);
        this.clearDraftAndQueuedIfUnchanged(key, request.value);
      }
    },
    clearDraftAndQueuedIfUnchanged(key: string, value: string) {
      if (this.draftCellValues.get(key) === value) {
        this.draftCellValues.delete(key);
      }
      if (this.queuedCellSaves.get(key)?.value === value) {
        this.queuedCellSaves.delete(key);
      }
    },
    isIdle() {
      return this.queuedCellSaves.size === 0 && this.activeCellSaves.size === 0;
    },
    schedulePendingSave(callbacks: PendingCellSaveCallbacks, debounceMs: number) {
      clearSchedulerTimer();
      this.setPhase("debouncing");
      const generation = schedulerGeneration;
      debounceTimer = setTimeout(() => {
        if (generation !== schedulerGeneration) return;
        debounceTimer = null;
        this.startPendingSave(callbacks);
      }, debounceMs);
    },
    startPendingSave(callbacks: PendingCellSaveCallbacks) {
      if (pendingSavePromise) {
        return;
      }

      this.setPhase("saving");
      const generation = schedulerGeneration;
      pendingSavePromise = this.saveQueuedBatch(callbacks, generation).finally(() => {
        if (generation !== schedulerGeneration) return;
        pendingSavePromise = null;
        if (this.hasQueuedSaves && !debounceTimer) {
          this.startPendingSave(callbacks);
          return;
        }
        if (this.isIdle()) {
          this.setPhase("idle");
          callbacks.clearPendingContentChange();
        }
      });
    },
    async saveQueuedBatch(
      callbacks: PendingCellSaveCallbacks,
      generation: number
    ): Promise<boolean> {
      if (!this.hasQueuedSaves) {
        callbacks.clearPendingContentChange();
        return true;
      }

      const changes = this.takeQueuedBatch();

      try {
        await callbacks.commitBatch(changes);
        if (generation !== schedulerGeneration) return false;
        this.completeBatch(changes);
        callbacks.onBatchCommitted?.();
      } catch (error) {
        if (generation !== schedulerGeneration) return false;
        await callbacks.onCommitFailed?.(error);
        this.failBatch(changes);
        this.setPhase("failed", String(error));
        if (this.isIdle()) {
          callbacks.clearPendingContentChange();
        }
        return false;
      }

      if (this.isIdle()) {
        this.setPhase("idle");
        callbacks.clearPendingContentChange();
      }
      return true;
    },
    clearDebounceIfNoQueuedSaves() {
      if (this.hasQueuedSaves || !debounceTimer) {
        return;
      }
      clearSchedulerTimer();
      if (!this.hasActiveSaves) {
        this.setPhase("idle");
      }
    },
    clearPendingContentChangeIfIdle(clearPendingContentChange: () => void) {
      if (!this.hasPendingWork()) {
        this.setPhase("idle");
        clearPendingContentChange();
      }
    },
    async flushPendingCellChanges(callbacks: PendingCellSaveCallbacks): Promise<boolean> {
      if (debounceTimer) {
        clearSchedulerTimer();
        if (this.hasQueuedSaves) {
          this.setPhase("saving");
        }
      }

      while (true) {
        if (pendingSavePromise) {
          const saved = await pendingSavePromise;
          if (!saved) return false;
        } else if (this.hasQueuedSaves) {
          this.startPendingSave(callbacks);
          if (!pendingSavePromise) return false;
          const saved = await pendingSavePromise;
          if (!saved) return false;
        }

        if (debounceTimer) {
          clearSchedulerTimer();
        }
        if (this.hasQueuedSaves) {
          continue;
        }
        if (!pendingSavePromise) {
          return true;
        }
      }
    },
    releaseSchedulerIfIdle() {
      if (this.hasPendingWork()) {
        return;
      }
      clearSchedulerTimer();
      this.setPhase("idle");
    },
    reset() {
      resetScheduler();
      this.draftCellValues.clear();
      this.queuedCellSaves.clear();
      this.activeCellSaves.clear();
      this.phase = "idle";
      this.lastError = null;
    },
  },
});

function cellKey(request: Pick<CellSaveRequest, "sheetIndex" | "row" | "col">): string {
  return `${request.sheetIndex},${request.row},${request.col}`;
}

function clearSchedulerTimer() {
  if (debounceTimer) {
    clearTimeout(debounceTimer);
    debounceTimer = null;
  }
}

function resetScheduler() {
  schedulerGeneration += 1;
  clearSchedulerTimer();
  pendingSavePromise = null;
}
