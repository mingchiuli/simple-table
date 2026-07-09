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

type PendingCellSaveSchedulerState = {
  debounceTimer: ReturnType<typeof setTimeout> | null;
  pendingSavePromise: Promise<boolean> | null;
  generation: number;
  autosaveSuspendCount: number;
  lastCallbacks: PendingCellSaveCallbacks | null;
  lastDebounceMs: number;
};

const pendingSaveSchedulers = new WeakMap<object, PendingCellSaveSchedulerState>();

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
      const scheduler = schedulerFor(this);
      return !this.isIdle()
        || scheduler.pendingSavePromise !== null
        || scheduler.debounceTimer !== null;
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
      const scheduler = schedulerFor(this);
      scheduler.lastCallbacks = callbacks;
      scheduler.lastDebounceMs = debounceMs;
      clearSchedulerTimer(scheduler);
      this.setPhase("debouncing");
      if (scheduler.autosaveSuspendCount > 0) {
        return;
      }
      const generation = scheduler.generation;
      scheduler.debounceTimer = setTimeout(() => {
        if (generation !== scheduler.generation) return;
        scheduler.debounceTimer = null;
        this.startPendingSave(callbacks);
      }, debounceMs);
    },
    startPendingSave(callbacks: PendingCellSaveCallbacks) {
      const scheduler = schedulerFor(this);
      scheduler.lastCallbacks = callbacks;
      if (scheduler.pendingSavePromise || scheduler.autosaveSuspendCount > 0) {
        return;
      }

      this.setPhase("saving");
      const generation = scheduler.generation;
      scheduler.pendingSavePromise = this.saveQueuedBatch(callbacks, generation).finally(() => {
        if (generation !== scheduler.generation) return;
        scheduler.pendingSavePromise = null;
        if (this.hasQueuedSaves && !scheduler.debounceTimer) {
          this.startPendingSave(callbacks);
          return;
        }
        if (this.isIdle() && this.phase !== "failed") {
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
        if (generation !== schedulerFor(this).generation) return false;
        this.completeBatch(changes);
        callbacks.onBatchCommitted?.();
      } catch (error) {
        if (generation !== schedulerFor(this).generation) return false;
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
      const scheduler = schedulerFor(this);
      if (this.hasQueuedSaves || !scheduler.debounceTimer) {
        return;
      }
      clearSchedulerTimer(scheduler);
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
    suspendAutosave(): () => void {
      const scheduler = schedulerFor(this);
      const generation = scheduler.generation;
      let released = false;
      scheduler.autosaveSuspendCount += 1;
      clearSchedulerTimer(scheduler);

      return () => {
        if (released) return;
        released = true;
        if (generation !== scheduler.generation) return;

        scheduler.autosaveSuspendCount = Math.max(0, scheduler.autosaveSuspendCount - 1);
        if (
          scheduler.autosaveSuspendCount === 0
          && this.hasQueuedSaves
          && !scheduler.pendingSavePromise
          && !scheduler.debounceTimer
          && scheduler.lastCallbacks
        ) {
          this.schedulePendingSave(scheduler.lastCallbacks, scheduler.lastDebounceMs);
          return;
        }
        if (this.isIdle() && !scheduler.pendingSavePromise && !scheduler.debounceTimer) {
          this.setPhase("idle");
        }
      };
    },
    async flushPendingCellChanges(callbacks: PendingCellSaveCallbacks): Promise<boolean> {
      const scheduler = schedulerFor(this);
      scheduler.lastCallbacks = callbacks;
      if (scheduler.debounceTimer) {
        clearSchedulerTimer(scheduler);
        if (this.hasQueuedSaves) {
          this.setPhase("saving");
        }
      }

      while (true) {
        if (scheduler.pendingSavePromise) {
          const saved = await scheduler.pendingSavePromise;
          if (!saved) return false;
        } else if (this.hasQueuedSaves) {
          this.startPendingSave(callbacks);
          if (!scheduler.pendingSavePromise) return false;
          const saved = await scheduler.pendingSavePromise;
          if (!saved) return false;
        }

        if (scheduler.debounceTimer) {
          clearSchedulerTimer(scheduler);
        }
        if (this.hasQueuedSaves) {
          continue;
        }
        if (!scheduler.pendingSavePromise) {
          return true;
        }
      }
    },
    releaseSchedulerIfIdle() {
      if (this.hasPendingWork()) {
        return;
      }
      clearSchedulerTimer(schedulerFor(this));
      this.setPhase("idle");
    },
    reset() {
      resetScheduler(schedulerFor(this));
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

function clearSchedulerTimer(scheduler: PendingCellSaveSchedulerState) {
  if (scheduler.debounceTimer) {
    clearTimeout(scheduler.debounceTimer);
    scheduler.debounceTimer = null;
  }
}

function resetScheduler(scheduler: PendingCellSaveSchedulerState) {
  scheduler.generation += 1;
  clearSchedulerTimer(scheduler);
  scheduler.pendingSavePromise = null;
  scheduler.autosaveSuspendCount = 0;
  scheduler.lastCallbacks = null;
}

function schedulerFor(store: object): PendingCellSaveSchedulerState {
  let scheduler = pendingSaveSchedulers.get(store);
  if (!scheduler) {
    scheduler = {
      debounceTimer: null,
      pendingSavePromise: null,
      generation: 0,
      autosaveSuspendCount: 0,
      lastCallbacks: null,
      lastDebounceMs: 0,
    };
    pendingSaveSchedulers.set(store, scheduler);
  }
  return scheduler;
}
