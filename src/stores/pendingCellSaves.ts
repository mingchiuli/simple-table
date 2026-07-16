import type { CellValue } from "@/types";
import { cellToEditorString } from "@/utils/cellValue";
import { utf8ByteLength } from "@/utils/utf8";

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
export const MAX_CELL_CHANGES_PER_BATCH = 4_096;
export const MAX_CELL_TEXT_BYTES = 4 * 1024 * 1024;
export const MAX_BATCH_TEXT_BYTES = 8 * 1024 * 1024;
export const MAX_PENDING_CELL_CHANGES = MAX_CELL_CHANGES_PER_BATCH * 2;
export const MAX_PENDING_TEXT_BYTES = MAX_BATCH_TEXT_BYTES * 2;

export class PendingCellSaveLimitError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "PendingCellSaveLimitError";
  }
}

export const usePendingCellSavesStore = defineStore("pendingCellSaves", {
  state: () => ({
    draftCellValues: new Map<string, string>(),
    queuedCellSaves: new Map<string, CellSaveRequest>(),
    activeCellSaves: new Map<string, CellSaveRequest>(),
    pendingTextBytes: 0,
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
      const nextRequest = {
        ...request,
        oldValue: existing?.oldValue ?? active?.oldValue ?? request.oldValue,
      };
      const requestBytes = requestTextBytes(nextRequest);
      if (requestBytes > MAX_CELL_TEXT_BYTES) {
        throw new PendingCellSaveLimitError(
          `Cell text is ${requestBytes} bytes; the maximum is ${MAX_CELL_TEXT_BYTES} bytes.`
        );
      }
      const projectedChanges = this.queuedCellSaves.size
        + this.activeCellSaves.size
        + Number(existing === undefined);
      if (projectedChanges > MAX_PENDING_CELL_CHANGES) {
        throw new PendingCellSaveLimitError(
          `Too many cell changes are waiting to be saved; the maximum is ${MAX_PENDING_CELL_CHANGES}.`
        );
      }
      const projectedBytes = this.pendingTextBytes
        - (existing ? requestTextBytes(existing) : 0)
        + requestBytes;
      if (projectedBytes > MAX_PENDING_TEXT_BYTES) {
        throw new PendingCellSaveLimitError(
          `Pending cell text requires ${projectedBytes} bytes; the maximum is ${MAX_PENDING_TEXT_BYTES} bytes.`
        );
      }
      this.queuedCellSaves.set(key, nextRequest);
      this.pendingTextBytes = projectedBytes;
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
        this.queueSave(key, request);
        this.setDraft(key, value);
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

      this.queueSave(key, request);
      this.setDraft(key, value);
      return { queued: true, shouldMarkPending: true, shouldClearPendingIfIdle: false };
    },
    cancelDraft(key: string, requestForRevert?: CellSaveRequest): QueueDraftResult {
      this.clearDraft(key);
      this.dropQueued(key);
      if (!requestForRevert) {
        return { queued: false, shouldMarkPending: false, shouldClearPendingIfIdle: true };
      }
      this.queueSave(key, requestForRevert);
      this.setDraft(key, requestForRevert.value);
      return { queued: true, shouldMarkPending: true, shouldClearPendingIfIdle: false };
    },
    dropQueued(key: string) {
      const request = this.queuedCellSaves.get(key);
      if (!request) return;
      this.queuedCellSaves.delete(key);
      this.pendingTextBytes = Math.max(0, this.pendingTextBytes - requestTextBytes(request));
    },
    takeQueuedBatch(): CellSaveRequest[] {
      const batch: CellSaveRequest[] = [];
      let batchTextBytes = 0;
      for (const [key, request] of this.queuedCellSaves) {
        const requestBytes = requestTextBytes(request);
        if (
          batch.length >= MAX_CELL_CHANGES_PER_BATCH
          || batchTextBytes + requestBytes > MAX_BATCH_TEXT_BYTES
        ) break;

        this.queuedCellSaves.delete(key);
        this.pendingTextBytes = Math.max(0, this.pendingTextBytes - requestBytes);
        const previousActive = this.activeCellSaves.get(key);
        if (previousActive) {
          this.pendingTextBytes = Math.max(
            0,
            this.pendingTextBytes - requestTextBytes(previousActive)
          );
        }
        this.activeCellSaves.set(key, request);
        this.pendingTextBytes += requestBytes;
        batchTextBytes += requestBytes;
        batch.push(request);
      }
      return batch;
    },
    completeBatch(batch: CellSaveRequest[]) {
      for (const request of batch) {
        const key = cellKey(request);
        const active = this.activeCellSaves.get(key);
        if (active) {
          this.activeCellSaves.delete(key);
          this.pendingTextBytes = Math.max(0, this.pendingTextBytes - requestTextBytes(active));
        }
        if (this.draftCellValues.get(key) === request.value) {
          this.draftCellValues.delete(key);
        }
      }
    },
    failBatch(batch: CellSaveRequest[]) {
      for (const request of batch) {
        const key = cellKey(request);
        const active = this.activeCellSaves.get(key);
        if (active) {
          this.activeCellSaves.delete(key);
          this.pendingTextBytes = Math.max(0, this.pendingTextBytes - requestTextBytes(active));
        }
        this.clearDraftAndQueuedIfUnchanged(key, request.value);
      }
    },
    clearDraftAndQueuedIfUnchanged(key: string, value: string) {
      if (this.draftCellValues.get(key) === value) {
        this.draftCellValues.delete(key);
      }
      if (this.queuedCellSaves.get(key)?.value === value) {
        this.dropQueued(key);
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
          safeClearPendingContentChange(callbacks);
        }
      });
    },
    async saveQueuedBatch(
      callbacks: PendingCellSaveCallbacks,
      generation: number
    ): Promise<boolean> {
      if (!this.hasQueuedSaves) {
        safeClearPendingContentChange(callbacks);
        return true;
      }

      const changes = this.takeQueuedBatch();

      try {
        await callbacks.commitBatch(changes);
        if (generation !== schedulerFor(this).generation) return false;
        this.completeBatch(changes);
        try {
          callbacks.onBatchCommitted?.();
        } catch (error) {
          console.error("Pending cell save committed, but post-commit handling failed:", error);
        }
      } catch (error) {
        if (generation !== schedulerFor(this).generation) return false;
        let failureReason = error;
        try {
          await callbacks.onCommitFailed?.(error);
        } catch (failureHandlerError) {
          failureReason = failureHandlerError;
          console.error("Pending cell save failure handling failed:", failureHandlerError);
        }
        this.failBatch(changes);
        this.setPhase("failed", String(failureReason));
        if (this.isIdle()) {
          safeClearPendingContentChange(callbacks);
        }
        return false;
      }

      if (this.isIdle()) {
        this.setPhase("idle");
        safeClearPendingContentChange(callbacks);
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
        safeClearPendingContentChangeCallback(clearPendingContentChange);
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
    waitForInFlightSave(): Promise<boolean> {
      return schedulerFor(this).pendingSavePromise ?? Promise.resolve(true);
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
      this.pendingTextBytes = 0;
      this.phase = "idle";
      this.lastError = null;
    },
  },
});

function cellKey(request: Pick<CellSaveRequest, "sheetIndex" | "row" | "col">): string {
  return `${request.sheetIndex},${request.row},${request.col}`;
}

function requestTextBytes(request: CellSaveRequest): number {
  return utf8ByteLength(request.value);
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

function safeClearPendingContentChange(callbacks: PendingCellSaveCallbacks) {
  safeClearPendingContentChangeCallback(callbacks.clearPendingContentChange);
}

function safeClearPendingContentChangeCallback(clearPendingContentChange: () => void) {
  try {
    clearPendingContentChange();
  } catch (error) {
    console.error("Pending cell save state was updated, but dirty-state cleanup failed:", error);
  }
}
