import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useRecentFileUpdates } from "@/composables/useRecentFileUpdates";
import {
  defaultHistoryStatus,
  defaultRichProjection,
  defaultWorkbookCapabilities,
  readyFormulaStatus,
  type CellValue,
} from "@/types";
import { openResponseFromFileData, type FileData } from "@/test/documentFixtures";
import { openDocumentSession } from '@/test/documentSessionTestDriver';
import {
  createApplicationWorkspaceTestContext,
  type ApplicationWorkspaceTestContext,
} from '@/test/documentWorkspaceTestContext';
import type { FileOperationReceipt } from '@/types/fileRuntime';

let workspace: ApplicationWorkspaceTestContext;

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
  removeRecentFile: vi.fn().mockResolvedValue(undefined),
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
    workspace.runtime,
    openResponseFromFileData(fileData(fileName), editorSession),
    `/tmp/${fileName}`
  );
}

describe("useRecentFileUpdates", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    workspace = createApplicationWorkspaceTestContext();
    vi.clearAllMocks();
  });

  afterEach(() => workspace.application.dispose());

  it("passes the successful file operation receipt to the background update", async () => {
    const api = await import("@/api");
    openRecentTestDocument("old.xlsx", 1, 3);
    const { queueRecentFileEntryUpdate } = workspace.run(() => useRecentFileUpdates());
    const oldReceipt = recentReceipt("old.xlsx", '1', '3');

    queueRecentFileEntryUpdate(oldReceipt, "/original/old.xlsx");
    openRecentTestDocument("new.xlsx", 2, 0);

    await flushPromises();

    expect(api.addRecentFileWithThumbnail).toHaveBeenCalledWith(
      oldReceipt,
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
    const { queueRecentFileEntryUpdate } = workspace.run(() => useRecentFileUpdates());
    const bookReceipt = recentReceipt("book.xlsx", '1', '3');

    queueRecentFileEntryUpdate(bookReceipt, "/original/first.xlsx");
    for (let index = 0; index < 10_000; index += 1) {
      queueRecentFileEntryUpdate(bookReceipt, `/original/${index}.xlsx`);
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
      bookReceipt,
      "/original/9999.xlsx"
    );
  });
});

function recentReceipt(
  fileName: string,
  documentId: `${bigint}`,
  revision: `${bigint}`,
): FileOperationReceipt {
  return {
    kind: 'open',
    documentId,
    revision,
    path: `/tmp/${fileName}`,
    fileName,
  };
}
