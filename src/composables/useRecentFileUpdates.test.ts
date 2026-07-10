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
    openRecentTestDocument("old.xlsx", 1, 3);
    const { queueRecentFileEntryUpdate } = useRecentFileUpdates();

    queueRecentFileEntryUpdate("/original/old.xlsx");
    openRecentTestDocument("new.xlsx", 2, 0);

    await flushPromises();

    expect(api.addRecentFileWithThumbnail).toHaveBeenCalledWith(
      { documentId: 1, baseRevision: 3 },
      "/original/old.xlsx"
    );
  });
});
