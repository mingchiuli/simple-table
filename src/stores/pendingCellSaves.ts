import type {
  CellSaveRequest,
  CellSaveState,
  CellValue,
  PendingCellSavePhase,
  QueueDraftResult,
} from "@/types";
import { cellToEditorString } from "@/utils/cellValue";
import { utf8ByteLength } from "@/utils/utf8";
import { markRaw } from "vue";

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
    draftCellValues: serializableIndex<string>(),
    queuedCellSaves: serializableIndex<CellSaveRequest>(),
    activeCellSaves: serializableIndex<CellSaveRequest>(),
    draftVersion: 0,
    queuedSaveCount: 0,
    activeSaveCount: 0,
    pendingTextBytes: 0,
    phase: "idle" as PendingCellSavePhase,
    lastError: null as string | null,
  }),
  getters: {
    hasQueuedSaves: (state) => state.queuedSaveCount > 0,
    hasActiveSaves: (state) => state.activeSaveCount > 0,
  },
  actions: {
    stateFor(key: string): CellSaveState {
      return {
        key,
        draft: this.draftCellValues[key],
        queued: this.queuedCellSaves[key],
        active: this.activeCellSaves[key],
      };
    },
    setDraft(key: string, value: string) {
      if (this.draftCellValues[key] === value) return;
      this.draftCellValues[key] = value;
      this.draftVersion += 1;
    },
    clearDraft(key: string) {
      if (!(key in this.draftCellValues)) return;
      delete this.draftCellValues[key];
      this.draftVersion += 1;
    },
    setPhase(phase: PendingCellSavePhase, error: string | null = null) {
      this.phase = phase;
      this.lastError = error;
    },
    queueSave(key: string, request: CellSaveRequest) {
      const existing = this.queuedCellSaves[key];
      const active = this.activeCellSaves[key];
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
      const projectedChanges = this.queuedSaveCount
        + this.activeSaveCount
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
      this.queuedCellSaves[key] = nextRequest;
      if (existing === undefined) this.queuedSaveCount += 1;
      this.pendingTextBytes = projectedBytes;
    },
    applyDraft(
      key: string,
      request: CellSaveRequest,
      committedValue: CellValue
    ): QueueDraftResult {
      const active = this.activeCellSaves[key];
      const queued = this.queuedCellSaves[key];
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

      if (this.draftCellValues[key] === value && queued?.value === value) {
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
      const request = this.queuedCellSaves[key];
      if (!request) return;
      delete this.queuedCellSaves[key];
      this.queuedSaveCount = Math.max(0, this.queuedSaveCount - 1);
      this.pendingTextBytes = Math.max(0, this.pendingTextBytes - requestTextBytes(request));
    },
    takeQueuedBatch(): CellSaveRequest[] {
      const batch: CellSaveRequest[] = [];
      let batchTextBytes = 0;
      for (const key in this.queuedCellSaves) {
        const request = this.queuedCellSaves[key];
        if (!request) continue;
        const requestBytes = requestTextBytes(request);
        if (
          batch.length >= MAX_CELL_CHANGES_PER_BATCH
          || batchTextBytes + requestBytes > MAX_BATCH_TEXT_BYTES
        ) break;

        delete this.queuedCellSaves[key];
        this.queuedSaveCount = Math.max(0, this.queuedSaveCount - 1);
        this.pendingTextBytes = Math.max(0, this.pendingTextBytes - requestBytes);
        const previousActive = this.activeCellSaves[key];
        if (previousActive) {
          this.pendingTextBytes = Math.max(
            0,
            this.pendingTextBytes - requestTextBytes(previousActive)
          );
        }
        this.activeCellSaves[key] = request;
        if (!previousActive) this.activeSaveCount += 1;
        this.pendingTextBytes += requestBytes;
        batchTextBytes += requestBytes;
        batch.push(request);
      }
      return batch;
    },
    completeBatch(batch: CellSaveRequest[]) {
      for (const request of batch) {
        const key = cellKey(request);
        const active = this.activeCellSaves[key];
        if (active) {
          delete this.activeCellSaves[key];
          this.activeSaveCount = Math.max(0, this.activeSaveCount - 1);
          this.pendingTextBytes = Math.max(0, this.pendingTextBytes - requestTextBytes(active));
        }
        if (this.draftCellValues[key] === request.value) {
          delete this.draftCellValues[key];
          this.draftVersion += 1;
        }
      }
    },
    failBatch(batch: CellSaveRequest[]) {
      for (const request of batch) {
        const key = cellKey(request);
        const active = this.activeCellSaves[key];
        if (active) {
          delete this.activeCellSaves[key];
          this.activeSaveCount = Math.max(0, this.activeSaveCount - 1);
          this.pendingTextBytes = Math.max(0, this.pendingTextBytes - requestTextBytes(active));
        }
        this.clearDraftAndQueuedIfUnchanged(key, request.value);
      }
    },
    clearDraftAndQueuedIfUnchanged(key: string, value: string) {
      if (this.draftCellValues[key] === value) {
        delete this.draftCellValues[key];
        this.draftVersion += 1;
      }
      if (this.queuedCellSaves[key]?.value === value) {
        this.dropQueued(key);
      }
    },
    isIdle() {
      return this.queuedSaveCount === 0 && this.activeSaveCount === 0;
    },
    reset() {
      this.draftCellValues = serializableIndex<string>();
      this.queuedCellSaves = serializableIndex<CellSaveRequest>();
      this.activeCellSaves = serializableIndex<CellSaveRequest>();
      this.draftVersion += 1;
      this.queuedSaveCount = 0;
      this.activeSaveCount = 0;
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

function serializableIndex<T>(): Record<string, T> {
  return markRaw({} as Record<string, T>);
}
