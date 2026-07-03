import { beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { usePendingCellSavesStore } from "@/stores/pendingCellSaves";
import type { CellValue } from "@/types";

function text(value: string): CellValue {
  return { type: "cell", kind: "text", raw: value, display: value };
}

describe("pendingCellSaves store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
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
    expect(store.queuedCellSaves.get("0,0,0")?.value).toBe("1");
    expect(store.draftCellValues.get("0,0,0")).toBe("1");
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
    expect(store.queuedCellSaves.has("0,0,0")).toBe(false);
    expect(store.draftCellValues.has("0,0,0")).toBe(false);
  });
});
