import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import {
  MAX_BATCH_TEXT_BYTES,
  MAX_PENDING_CELL_CHANGES,
  MAX_CELL_TEXT_BYTES,
  MAX_PENDING_TEXT_BYTES,
  PendingCellSaveLimitError,
  usePendingCellSavesStore,
} from "@/stores/pendingCellSaves";
import { usePendingCellSaveCoordinator } from '@/composables/usePendingCellSaveCoordinator';
import type { CellValue } from "@/types";

function text(value: string): CellValue {
  return { type: "cell", kind: "text", raw: value, display: value };
}

describe("pendingCellSaves store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("queues a revert when the user changes back while a save is active", () => {
    const store = usePendingCellSavesStore();
    store.queueSave("0,0,0", {
      sheetIndex: 0,
      row: 0,
      col: 0,
      value: "2",
      oldValue: text("1"),
    });
    const [active] = store.takeQueuedBatch();

    const result = store.applyDraft(
      "0,0,0",
      { sheetIndex: 0, row: 0, col: 0, value: "1", oldValue: text("1") },
      text("1")
    );

    expect(active.value).toBe("2");
    expect(result.queued).toBe(true);
    expect(store.queuedCellSaves["0,0,0"]?.value).toBe("1");
    expect(store.draftCellValues["0,0,0"]).toBe("1");
  });

  it("keeps pending edit state JSON serializable", () => {
    const store = usePendingCellSavesStore();
    store.queueSave("0,0,0", {
      sheetIndex: 0,
      row: 0,
      col: 0,
      value: "draft",
      oldValue: text("old"),
    });

    const serialized = JSON.parse(JSON.stringify(store.$state));
    expect(serialized.queuedCellSaves["0,0,0"].value).toBe("draft");
  });

  it("drops a queued draft when inline editing is cancelled before save starts", () => {
    const store = usePendingCellSavesStore();
    store.applyDraft(
      "0,0,0",
      { sheetIndex: 0, row: 0, col: 0, value: "draft", oldValue: text("old") },
      text("old")
    );

    const result = store.cancelDraft("0,0,0");

    expect(result.shouldClearPendingIfIdle).toBe(true);
    expect(store.queuedCellSaves["0,0,0"]).toBeUndefined();
    expect(store.draftCellValues["0,0,0"]).toBeUndefined();
  });

  it("commits oversized pending changes as consecutive bounded batches", async () => {
    const store = usePendingCellSavesStore();
    const committedBatchSizes: number[] = [];
    for (let row = 0; row <= 4_096; row += 1) {
      store.queueSave(`0,${row},0`, {
        sheetIndex: 0,
        row,
        col: 0,
        value: String(row),
        oldValue: text(""),
      });
    }

    const flushed = await usePendingCellSaveCoordinator().flushPendingCellChanges({
      commitBatch: async (changes) => {
        committedBatchSizes.push(changes.length);
      },
      clearPendingContentChange: () => undefined,
    });

    expect(flushed).toBe(true);
    expect(committedBatchSizes).toEqual([4_096, 1]);
    expect(store.isIdle()).toBe(true);
  });

  it("forms batches by UTF-8 bytes without copying the complete queue", () => {
    const store = usePendingCellSavesStore();
    const value = "x".repeat(3 * 1024 * 1024);
    for (let row = 0; row < 3; row += 1) {
      store.queueSave(`0,${row},0`, {
        sheetIndex: 0,
        row,
        col: 0,
        value,
        oldValue: text(""),
      });
    }

    const first = store.takeQueuedBatch();
    const second = store.takeQueuedBatch();

    expect(first).toHaveLength(2);
    expect(first.reduce((bytes, change) => bytes + change.value.length, 0))
      .toBeLessThanOrEqual(MAX_BATCH_TEXT_BYTES);
    expect(second).toHaveLength(1);
  });

  it("rejects a cell before replacing an accepted draft when its text is too large", () => {
    const store = usePendingCellSavesStore();
    store.applyDraft(
      "0,0,0",
      { sheetIndex: 0, row: 0, col: 0, value: "accepted", oldValue: text("") },
      text("")
    );

    expect(() => store.applyDraft(
      "0,0,0",
      {
        sheetIndex: 0,
        row: 0,
        col: 0,
        value: "x".repeat(MAX_CELL_TEXT_BYTES + 1),
        oldValue: text(""),
      },
      text("")
    )).toThrow(PendingCellSaveLimitError);
    expect(store.draftCellValues["0,0,0"]).toBe("accepted");
    expect(store.queuedCellSaves["0,0,0"]?.value).toBe("accepted");
  });

  it("bounds all queued and active cell text", () => {
    const store = usePendingCellSavesStore();
    const value = "x".repeat(MAX_CELL_TEXT_BYTES);
    const accepted = MAX_PENDING_TEXT_BYTES / MAX_CELL_TEXT_BYTES;
    for (let row = 0; row < accepted; row += 1) {
      store.queueSave(`0,${row},0`, {
        sheetIndex: 0,
        row,
        col: 0,
        value,
        oldValue: text(""),
      });
    }

    expect(store.pendingTextBytes).toBe(MAX_PENDING_TEXT_BYTES);
    expect(() => store.queueSave(`0,${accepted},0`, {
      sheetIndex: 0,
      row: accepted,
      col: 0,
      value: "x",
      oldValue: text(""),
    })).toThrow(PendingCellSaveLimitError);
  });

  it("releases pending byte accounting after a batch completes", () => {
    const store = usePendingCellSavesStore();
    store.queueSave("0,0,0", {
      sheetIndex: 0,
      row: 0,
      col: 0,
      value: "中文",
      oldValue: text(""),
    });
    const batch = store.takeQueuedBatch();

    expect(store.pendingTextBytes).toBe(6);
    store.completeBatch(batch);
    expect(store.pendingTextBytes).toBe(0);
  });

  it("bounds the total number of queued and active changes", () => {
    const store = usePendingCellSavesStore();
    for (let row = 0; row < MAX_PENDING_CELL_CHANGES; row += 1) {
      store.queueSave(`0,${row},0`, {
        sheetIndex: 0,
        row,
        col: 0,
        value: "",
        oldValue: text(""),
      });
    }

    expect(() => store.queueSave(`0,${MAX_PENDING_CELL_CHANGES},0`, {
      sheetIndex: 0,
      row: MAX_PENDING_CELL_CHANGES,
      col: 0,
      value: "",
      oldValue: text(""),
    })).toThrow(PendingCellSaveLimitError);
  });

  it("owns the debounce scheduler at store scope", async () => {
    vi.useFakeTimers();
    const store = usePendingCellSavesStore();
    const committed: string[] = [];
    let cleared = 0;
    store.queueSave("0,0,0", {
      sheetIndex: 0,
      row: 0,
      col: 0,
      value: "draft",
      oldValue: text("old"),
    });

    usePendingCellSaveCoordinator().schedulePendingSave(
      {
        commitBatch: async (changes) => {
          committed.push(changes[0].value);
        },
        clearPendingContentChange: () => {
          cleared += 1;
        },
      },
      100
    );

    expect(store.phase).toBe("debouncing");
    await vi.advanceTimersByTimeAsync(100);

    expect(committed).toEqual(["draft"]);
    expect(store.phase).toBe("idle");
    expect(store.isIdle()).toBe(true);
    expect(cleared).toBeGreaterThan(0);
  });

  it("reset cancels scheduled debounce work", async () => {
    vi.useFakeTimers();
    const store = usePendingCellSavesStore();
    const committed: string[] = [];
    store.queueSave("0,0,0", {
      sheetIndex: 0,
      row: 0,
      col: 0,
      value: "discarded",
      oldValue: text("old"),
    });

    usePendingCellSaveCoordinator().schedulePendingSave(
      {
        commitBatch: async (changes) => {
          committed.push(changes[0].value);
        },
        clearPendingContentChange: () => undefined,
      },
      100
    );

    usePendingCellSaveCoordinator().reset();
    await vi.advanceTimersByTimeAsync(100);

    expect(committed).toEqual([]);
    expect(store.phase).toBe("idle");
    expect(usePendingCellSaveCoordinator().hasPendingWork()).toBe(false);
  });

  it("flushes through the store scheduler and waits for active saves", async () => {
    const store = usePendingCellSavesStore();
    let releaseSave!: () => void;
    const activeSave = new Promise<void>((resolve) => {
      releaseSave = resolve;
    });
    let started = false;

    store.queueSave("0,0,0", {
      sheetIndex: 0,
      row: 0,
      col: 0,
      value: "draft",
      oldValue: text("old"),
    });

    const flush = usePendingCellSaveCoordinator().flushPendingCellChanges({
      commitBatch: async () => {
        started = true;
        await activeSave;
      },
      clearPendingContentChange: () => undefined,
    });

    await Promise.resolve();
    expect(started).toBe(true);
    expect(store.hasActiveSaves).toBe(true);

    releaseSave();
    await expect(flush).resolves.toBe(true);
    expect(store.isIdle()).toBe(true);
  });

  it("reports flush failure when a queued batch cannot be committed", async () => {
    const store = usePendingCellSavesStore();
    let committed = false;
    let failed: unknown = null;
    store.queueSave("0,0,0", {
      sheetIndex: 0,
      row: 0,
      col: 0,
      value: "draft",
      oldValue: text("old"),
    });

    const flushed = await usePendingCellSaveCoordinator().flushPendingCellChanges({
      commitBatch: async () => {
        throw new Error("stale projection");
      },
      clearPendingContentChange: () => undefined,
      onBatchCommitted: () => {
        committed = true;
      },
      onCommitFailed: (error) => {
        failed = error;
      },
    });

    expect(flushed).toBe(false);
    expect(committed).toBe(false);
    expect(String(failed)).toContain("stale projection");
    expect(store.phase).toBe("failed");
  });

  it("clears active saves when commit failure handling throws", async () => {
    const store = usePendingCellSavesStore();
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    store.queueSave("0,0,0", {
      sheetIndex: 0,
      row: 0,
      col: 0,
      value: "draft",
      oldValue: text("old"),
    });

    const flushed = await usePendingCellSaveCoordinator().flushPendingCellChanges({
      commitBatch: async () => {
        throw new Error("backend failed");
      },
      clearPendingContentChange: () => undefined,
      onCommitFailed: () => {
        throw new Error("refresh failed");
      },
    });

    try {
      expect(flushed).toBe(false);
      expect(store.hasActiveSaves).toBe(false);
      expect(usePendingCellSaveCoordinator().hasPendingWork()).toBe(false);
      expect(store.phase).toBe("failed");
      expect(store.lastError).toContain("refresh failed");
      expect(consoleError).toHaveBeenCalled();
    } finally {
      consoleError.mockRestore();
    }
  });

  it("does not reject saves when dirty-state cleanup throws", async () => {
    const store = usePendingCellSavesStore();
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    store.queueSave("0,0,0", {
      sheetIndex: 0,
      row: 0,
      col: 0,
      value: "draft",
      oldValue: text("old"),
    });

    const flushed = await usePendingCellSaveCoordinator().flushPendingCellChanges({
      commitBatch: async () => undefined,
      clearPendingContentChange: () => {
        throw new Error("dirty cleanup failed");
      },
    });

    try {
      expect(flushed).toBe(true);
      expect(usePendingCellSaveCoordinator().hasPendingWork()).toBe(false);
      expect(store.phase).toBe("idle");
      expect(consoleError).toHaveBeenCalled();
    } finally {
      consoleError.mockRestore();
    }
  });

  it("does not throw when idle dirty-state cleanup fails", () => {
    const store = usePendingCellSavesStore();
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);

    try {
      expect(() =>
        usePendingCellSaveCoordinator().clearPendingContentChangeIfIdle(() => {
          throw new Error("dirty cleanup failed");
        })
      ).not.toThrow();
      expect(store.phase).toBe("idle");
      expect(consoleError).toHaveBeenCalled();
    } finally {
      consoleError.mockRestore();
    }
  });

  it("suspends debounce autosave without dropping queued drafts", async () => {
    vi.useFakeTimers();
    const store = usePendingCellSavesStore();
    const committed: string[] = [];
    store.queueSave("0,0,0", {
      sheetIndex: 0,
      row: 0,
      col: 0,
      value: "draft",
      oldValue: text("old"),
    });

    const resume = usePendingCellSaveCoordinator().suspendAutosave();
    usePendingCellSaveCoordinator().schedulePendingSave(
      {
        commitBatch: async (changes) => {
          committed.push(changes[0].value);
        },
        clearPendingContentChange: () => undefined,
      },
      100
    );
    await vi.advanceTimersByTimeAsync(100);

    expect(committed).toEqual([]);
    expect(store.queuedCellSaves["0,0,0"]?.value).toBe("draft");

    resume();
    await vi.advanceTimersByTimeAsync(100);

    expect(committed).toEqual(["draft"]);
    expect(store.isIdle()).toBe(true);
  });

  it("does not resume suspended autosave after reset", async () => {
    vi.useFakeTimers();
    const store = usePendingCellSavesStore();
    const committed: string[] = [];
    store.queueSave("0,0,0", {
      sheetIndex: 0,
      row: 0,
      col: 0,
      value: "draft",
      oldValue: text("old"),
    });

    const resume = usePendingCellSaveCoordinator().suspendAutosave();
    usePendingCellSaveCoordinator().schedulePendingSave(
      {
        commitBatch: async (changes) => {
          committed.push(changes[0].value);
        },
        clearPendingContentChange: () => undefined,
      },
      100
    );

    usePendingCellSaveCoordinator().reset();
    resume();
    await vi.advanceTimersByTimeAsync(100);

    expect(committed).toEqual([]);
    expect(usePendingCellSaveCoordinator().hasPendingWork()).toBe(false);
  });
});
