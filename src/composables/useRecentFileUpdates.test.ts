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
} from "@/types";
import { openResponseFromFileData, type FileData } from "@/test/documentFixtures";
import { openDocumentSession } from '@/test/documentSessionTestDriver';

vi.mock("@/api", () => ({
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

async function flushPromises() {
  for (let i = 0; i < 8; i += 1) {
    await Promise.resolve();
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
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

function openRecentTestDocument(
  fileName: string,
  documentId: number | string,
  revision: number | string
) {
  const editorSession = {
      documentId: String(documentId) as `${bigint}`,
      revision: String(revision) as `${bigint}`,
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
    openResponseFromFileData(fileData(fileName), editorSession),
    `/tmp/${fileName}`
  );
}

describe("useRecentFileUpdates", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("captures the active document context before running background recent updates", async () => {
    const api = await import("@/api");
    openRecentTestDocument("old.xlsx", 1, 3);
    const { queueRecentFileEntryUpdate } = useRecentFileUpdates();

    queueRecentFileEntryUpdate("/original/old.xlsx");
    openRecentTestDocument("new.xlsx", 2, 0);

    await flushPromises();

    expect(api.addRecentFileWithThumbnail).toHaveBeenCalledWith(
      { documentId: '1', baseRevision: '3' },
      "/original/old.xlsx"
    );
  });

  it("retains only the latest recent update while one is active", async () => {
    const api = await import("@/api");
    const firstUpdate = deferred<Awaited<ReturnType<typeof api.addRecentFileWithThumbnail>>>();
    vi.mocked(api.addRecentFileWithThumbnail)
      .mockReturnValueOnce(firstUpdate.promise)
      .mockResolvedValue({
        id: "recent",
        path: "/tmp/book.xlsx",
        fileName: "book.xlsx",
        lastOpened: 1,
        fileSize: 42,
        storageType: "desktopPath",
      });
    openRecentTestDocument("book.xlsx", 1, 3);
    const { queueRecentFileEntryUpdate } = useRecentFileUpdates();

    queueRecentFileEntryUpdate("/original/first.xlsx");
    for (let index = 0; index < 10_000; index += 1) {
      queueRecentFileEntryUpdate(`/original/${index}.xlsx`);
    }
    await flushPromises();

    expect(api.addRecentFileWithThumbnail).toHaveBeenCalledTimes(1);

    firstUpdate.resolve({
      id: "first",
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      lastOpened: 1,
      fileSize: 42,
      storageType: "desktopPath",
    });
    await flushPromises();

    expect(api.addRecentFileWithThumbnail).toHaveBeenCalledTimes(2);
    expect(api.addRecentFileWithThumbnail).toHaveBeenLastCalledWith(
      { documentId: "1", baseRevision: "3" },
      "/original/9999.xlsx"
    );
  });
});
