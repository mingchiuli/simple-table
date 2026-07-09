import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useDocumentReplacementGuard } from "@/composables/useDocumentReplacementGuard";
import { useDocumentStatusStore } from "@/stores/documentStatus";
import { usePendingCellSavesStore } from "@/stores/pendingCellSaves";
import { useDocumentSessionStore } from "@/stores/documentSession";
import {
  defaultHistoryStatus,
  defaultRichProjection,
  defaultWorkbookCapabilities,
  readyFormulaStatus,
  type CellValue,
} from "@/types";

const unsavedChanges = vi.hoisted(() => ({
  confirmDiscardUnsavedChanges: vi.fn(),
}));

vi.mock("@/utils/unsavedChanges", async () => {
  const actual = await vi.importActual<typeof import("@/utils/unsavedChanges")>(
    "@/utils/unsavedChanges"
  );
  return {
    ...actual,
    confirmDiscardUnsavedChanges: unsavedChanges.confirmDiscardUnsavedChanges,
  };
});

function text(value: string): CellValue {
  return { type: "cell", kind: "text", raw: value, display: value };
}

function openTestDocument(documentId = 1) {
  useDocumentSessionStore().openDocumentResponse({
    fileData: {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [{ name: "Sheet1", rows: [[text("old")]], merges: [], rich: defaultRichProjection() }],
    },
    editorSession: {
      documentId,
      revision: 0,
      formulaStatus: readyFormulaStatus(),
      capabilities: defaultWorkbookCapabilities(),
      editorState: {
        canUndo: false,
        canRedo: false,
        isDirty: false,
        history: defaultHistoryStatus(),
      },
    },
  }, "/tmp/book.xlsx");
}

function queueDraftWithAutosave(committed: string[]) {
  const pendingStore = usePendingCellSavesStore();
  pendingStore.queueSave("0,0,0", {
    sheetIndex: 0,
    row: 0,
    col: 0,
    value: "draft",
    oldValue: text("old"),
  });
  pendingStore.schedulePendingSave(
    {
      commitBatch: async (changes) => {
        committed.push(changes[0].value);
      },
      clearPendingContentChange: () => undefined,
    },
    100
  );
  return pendingStore;
}

async function flushPromises() {
  for (let i = 0; i < 8; i += 1) {
    await Promise.resolve();
  }
}

describe("useDocumentReplacementGuard", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("pauses pending autosave until a cancelled replacement resumes it", async () => {
    const statusStore = useDocumentStatusStore();
    const committed: string[] = [];
    const pendingStore = queueDraftWithAutosave(committed);
    statusStore.markPendingContentChange();
    unsavedChanges.confirmDiscardUnsavedChanges.mockResolvedValue(true);

    const replacement = await useDocumentReplacementGuard().beginDocumentReplacement();
    await vi.advanceTimersByTimeAsync(100);

    expect(replacement).not.toBeNull();
    expect(committed).toEqual([]);
    expect(pendingStore.queuedCellSaves.get("0,0,0")?.value).toBe("draft");

    replacement?.cancel();
    await vi.advanceTimersByTimeAsync(100);

    expect(committed).toEqual(["draft"]);
  });

  it("pauses pending autosave while discard confirmation is still open", async () => {
    const statusStore = useDocumentStatusStore();
    const committed: string[] = [];
    const pendingStore = queueDraftWithAutosave(committed);
    statusStore.markPendingContentChange();
    let resolveConfirm!: (confirmed: boolean) => void;
    unsavedChanges.confirmDiscardUnsavedChanges.mockReturnValue(
      new Promise<boolean>((resolve) => {
        resolveConfirm = resolve;
      })
    );

    const replacementPromise = useDocumentReplacementGuard().beginDocumentReplacement();
    await vi.advanceTimersByTimeAsync(100);

    expect(committed).toEqual([]);
    expect(pendingStore.queuedCellSaves.get("0,0,0")?.value).toBe("draft");

    resolveConfirm(false);
    await expect(replacementPromise).resolves.toBeNull();
    await vi.advanceTimersByTimeAsync(100);

    expect(committed).toEqual(["draft"]);
  });

  it("drops pending work when a replacement is committed", async () => {
    const statusStore = useDocumentStatusStore();
    const committed: string[] = [];
    const pendingStore = queueDraftWithAutosave(committed);
    statusStore.markPendingContentChange();
    unsavedChanges.confirmDiscardUnsavedChanges.mockResolvedValue(true);

    const replacement = await useDocumentReplacementGuard().beginDocumentReplacement();
    replacement?.commit();
    await vi.advanceTimersByTimeAsync(100);

    expect(committed).toEqual([]);
    expect(statusStore.hasPendingContentChange).toBe(false);
    expect(pendingStore.hasPendingWork()).toBe(false);
  });

  it("waits for active document mutations before allowing confirmed discard replacement", async () => {
    const statusStore = useDocumentStatusStore();
    const documentSessionStore = useDocumentSessionStore();
    openTestDocument(1);
    statusStore.markPendingContentChange();
    unsavedChanges.confirmDiscardUnsavedChanges.mockResolvedValue(true);
    let releaseMutation!: () => void;
    let replacementResolved = false;
    void documentSessionStore.enqueueDocumentMutation(1, async () => {
      await new Promise<void>((resolve) => {
        releaseMutation = resolve;
      });
    });

    const replacementPromise = useDocumentReplacementGuard()
      .beginDocumentReplacement()
      .then((replacement) => {
        replacementResolved = true;
        return replacement;
      });

    await flushPromises();

    expect(replacementResolved).toBe(false);

    releaseMutation();
    const replacement = await replacementPromise;

    expect(replacement).not.toBeNull();
  });

});
