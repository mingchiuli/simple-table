import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useDocumentReplacementGuard } from "@/composables/useDocumentReplacementGuard";
import { useDocumentStatusStore } from "@/stores/documentStatus";
import { usePendingCellSavesStore } from "@/stores/pendingCellSaves";
import { useDocumentSessionStore } from "@/stores/documentSession";
import { useDocumentSessionCoordinator } from '@/composables/useDocumentSessionCoordinator';
import { usePendingCellSaveCoordinator } from '@/composables/usePendingCellSaveCoordinator';
import {
  defaultHistoryStatus,
  defaultRichProjection,
  defaultWorkbookCapabilities,
  readyFormulaStatus,
  type CellValue,
} from "@/types";
import { openResponseFromFileData } from "@/test/documentFixtures";
import { openDocumentSession } from '@/test/documentSessionTestDriver';

const unsavedChanges = vi.hoisted(() => ({
  confirmDiscardUnsavedChanges: vi.fn(),
}));

vi.mock("@/composables/unsavedChangesDialog", async () => {
  const actual = await vi.importActual<typeof import("@/composables/unsavedChangesDialog")>(
    "@/composables/unsavedChangesDialog"
  );
  return {
    ...actual,
    confirmDiscardUnsavedChanges: unsavedChanges.confirmDiscardUnsavedChanges,
  };
});

function text(value: string): CellValue {
  return { type: "cell", kind: "text", raw: value, display: value };
}

function openTestDocument(documentId: number | string = '1') {
  const fileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [{ name: "Sheet1", rows: [[text("old")]], merges: [], rich: defaultRichProjection() }],
    };
  const editorSession = {
        documentId: String(documentId) as `${bigint}`,
      revision: '0' as const,
      formulaStatus: readyFormulaStatus(),
      capabilities: defaultWorkbookCapabilities(),
      editorState: {
        canUndo: false,
        canRedo: false,
        isDirty: false,
        history: defaultHistoryStatus(),
      },
    };
  openDocumentSession(
    useDocumentSessionStore(),
    openResponseFromFileData(fileData, editorSession),
    "/tmp/book.xlsx"
  );
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
  usePendingCellSaveCoordinator().schedulePendingSave(
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

function deferred<T = void>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
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
    expect(pendingStore.queuedCellSaves["0,0,0"]?.value).toBe("draft");

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
    expect(pendingStore.queuedCellSaves["0,0,0"]?.value).toBe("draft");

    resolveConfirm(false);
    await expect(replacementPromise).resolves.toBeNull();
    await vi.advanceTimersByTimeAsync(100);

    expect(committed).toEqual(["draft"]);
  });

  it("resumes pending autosave if discard confirmation fails unexpectedly", async () => {
    const statusStore = useDocumentStatusStore();
    const committed: string[] = [];
    queueDraftWithAutosave(committed);
    statusStore.markPendingContentChange();
    unsavedChanges.confirmDiscardUnsavedChanges.mockRejectedValue(new Error("dialog failed"));

    await expect(useDocumentReplacementGuard().beginDocumentReplacement()).rejects.toThrow(
      "dialog failed"
    );
    await vi.advanceTimersByTimeAsync(100);

    expect(committed).toEqual(["draft"]);
  });

  it("drops pending work when a replacement is committed", async () => {
    const statusStore = useDocumentStatusStore();
    const committed: string[] = [];
    queueDraftWithAutosave(committed);
    statusStore.markPendingContentChange();
    unsavedChanges.confirmDiscardUnsavedChanges.mockResolvedValue(true);

    const replacement = await useDocumentReplacementGuard().beginDocumentReplacement();
    replacement?.commit();
    await vi.advanceTimersByTimeAsync(100);

    expect(committed).toEqual([]);
    expect(statusStore.hasPendingContentChange).toBe(false);
    expect(usePendingCellSaveCoordinator().hasPendingWork()).toBe(false);
  });

  it("waits for active document mutations before allowing confirmed discard replacement", async () => {
    const statusStore = useDocumentStatusStore();
    openTestDocument(1);
    statusStore.markPendingContentChange();
    unsavedChanges.confirmDiscardUnsavedChanges.mockResolvedValue(true);
    let releaseMutation!: () => void;
    let replacementResolved = false;
    void useDocumentSessionCoordinator().enqueueDocumentMutation('1', async () => {
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

  it("waits for in-flight cell saves before allowing confirmed discard replacement", async () => {
    const statusStore = useDocumentStatusStore();
    const pendingStore = usePendingCellSavesStore();
    const enqueueGate = deferred();
    const mutationGate = deferred();
    openTestDocument(1);
    statusStore.markPendingContentChange();
    unsavedChanges.confirmDiscardUnsavedChanges.mockResolvedValue(true);
    let saveStarted = false;
    let replacementResolved = false;

    pendingStore.queueSave("0,0,0", {
      sheetIndex: 0,
      row: 0,
      col: 0,
      value: "draft",
      oldValue: text("old"),
    });
    usePendingCellSaveCoordinator().startPendingSave({
      commitBatch: async () => {
        saveStarted = true;
        await enqueueGate.promise;
        await useDocumentSessionCoordinator().enqueueDocumentMutation('1', async () => {
          await mutationGate.promise;
        });
      },
      clearPendingContentChange: () => undefined,
    });
    await flushPromises();

    expect(saveStarted).toBe(true);

    const replacementPromise = useDocumentReplacementGuard()
      .beginDocumentReplacement()
      .then((replacement) => {
        replacementResolved = true;
        return replacement;
      });
    await flushPromises();

    expect(replacementResolved).toBe(false);

    enqueueGate.resolve();
    await flushPromises();

    expect(replacementResolved).toBe(false);

    mutationGate.resolve();
    const replacement = await replacementPromise;

    expect(replacement).not.toBeNull();
  });

});
