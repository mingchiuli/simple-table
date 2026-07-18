import type {
  CellSaveRequest,
  CellSaveState,
  CellValue,
  PendingCellSavePhase,
  QueueDraftResult,
} from "@/types";
import { cellToEditorString } from "@/utils/cellValue";
import { utf8ByteLength } from "@/utils/utf8";

export type {
  CellSaveRequest,
  CellSaveState,
  PendingCellSaveCallbacks,
  PendingCellSavePhase,
  QueueDraftResult,
} from '@/types';
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
    reset() {
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
