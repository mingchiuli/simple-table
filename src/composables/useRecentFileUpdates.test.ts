import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useRecentFileUpdates } from "@/composables/useRecentFileUpdates";
import { useDocumentSessionStore } from "@/stores/documentSession";
import {
  defaultHistoryStatus,
  defaultRichProjection,
  defaultWorkbookCapabilities,
  readyFormulaStatus,
  type CellValue,
  type FileData,
} from "@/types";

const platformMocks = vi.hoisted(() => ({
  getStorageType: vi.fn(),
}));

vi.mock("@/platform", () => ({
  getStorageType: platformMocks.getStorageType,
}));

vi.mock("@/api", () => ({
  getFileSize: vi.fn().mockResolvedValue(42),
  addRecentFileWithThumbnail: vi.fn().mockResolvedValue({
    id: "recent",
    path: "/tmp/book.xlsx",
    fileName: "book.xlsx",
    lastOpened: 1,
    fileSize: 42,
    storageType: "desktopPath",
  }),
  getRecentFiles: vi.fn().mockResolvedValue([]),
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

async function flushPromises() {
  for (let i = 0; i < 8; i += 1) {
    await Promise.resolve();
  }
}

function text(value: string): CellValue {
  return { type: "cell", kind: "text", raw: value, display: value };
}

function fileData(fileName: string): FileData {
  return {
    path: `/tmp/${fileName}`,
    fileName,
    sheets: [
      {
        name: "Sheet1",
        rows: [[text(fileName)]],
        merges: [],
        rich: defaultRichProjection(),
      },
    ],
  };
}

function openRecentTestDocument(fileName: string, documentId: number, revision: number) {
  useDocumentSessionStore().openDocumentResponse({
    fileData: fileData(fileName),
    editorSession: {
      documentId,
      revision,
      formulaStatus: readyFormulaStatus(),
      capabilities: defaultWorkbookCapabilities(),
      editorState: {
        canUndo: false,
        canRedo: false,
        isDirty: false,
        history: defaultHistoryStatus(),
      },
    },
  }, `/tmp/${fileName}`);
}

describe("useRecentFileUpdates", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("captures the active document context before running background recent updates", async () => {
    const api = await import("@/api");
    const storageType = deferred<"desktopPath">();
    platformMocks.getStorageType.mockReturnValue(storageType.promise);
    openRecentTestDocument("old.xlsx", 1, 3);
    const { queueRecentFileEntryUpdate } = useRecentFileUpdates();

    queueRecentFileEntryUpdate("/tmp/old.xlsx", "old.xlsx");
    openRecentTestDocument("new.xlsx", 2, 0);

    storageType.resolve("desktopPath");
    await flushPromises();

    expect(api.addRecentFileWithThumbnail).toHaveBeenCalledWith(
      "/tmp/old.xlsx",
      "old.xlsx",
      42,
      "desktopPath",
      undefined,
      { documentId: 1, baseRevision: 3 }
    );
  });
});
