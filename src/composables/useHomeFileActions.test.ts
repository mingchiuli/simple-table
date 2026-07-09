import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useHomeFileActions } from "@/composables/useHomeFileActions";
import { useDocumentSessionStore } from "@/stores/documentSession";
import { useDocumentStatusStore } from "@/stores/documentStatus";
import { usePendingCellSavesStore } from "@/stores/pendingCellSaves";
import {
  defaultHistoryStatus,
  defaultRichProjection,
  defaultWorkbookCapabilities,
  readyFormulaStatus,
  type CellValue,
  type OpenDocumentResponse,
  type RecentFile,
} from "@/types";

const spreadsheetFormats = vi.hoisted(() => ({
  defaultSpreadsheetExtension: vi.fn(),
}));

vi.mock("vue-router", () => ({
  useRouter: () => ({
    push: vi.fn(),
  }),
}));

vi.mock("element-plus", () => ({
  ElMessage: {
    error: vi.fn(),
  },
}));

vi.mock("@/api", () => ({
  addRecentFileWithThumbnail: vi.fn().mockResolvedValue(undefined),
  checkFileExists: vi.fn(),
  getFileSize: vi.fn().mockResolvedValue(42),
  getSpreadsheetFormatOptions: vi.fn().mockResolvedValue({
    defaultExtension: "xlsx",
    supportedExtensions: ["xlsx", "csv"],
  }),
  initFile: vi.fn(),
  removeRecentFile: vi.fn(),
}));

vi.mock("@/platform", () => ({
  discardOpenFileSelection: vi.fn(),
  getFileName: vi.fn(),
  getStorageType: vi.fn().mockResolvedValue("desktopPath"),
  pickOpenFile: vi.fn(),
  readFile: vi.fn(),
}));

vi.mock("@/utils/unsavedChanges", async () => {
  const actual = await vi.importActual<typeof import("@/utils/unsavedChanges")>(
    "@/utils/unsavedChanges"
  );
  return {
    ...actual,
    confirmDiscardUnsavedChanges: vi.fn(),
  };
});

vi.mock("@/utils/spreadsheetFormats", () => spreadsheetFormats);

function text(value: string): CellValue {
  return { type: "cell", kind: "text", raw: value, display: value };
}

function openedResponse(fileName = "book.xlsx", documentId = 1): OpenDocumentResponse {
  return {
    fileData: {
      path: `/tmp/${fileName}`,
      fileName,
      sheets: [
        {
          name: "Sheet1",
          rows: [[text("opened")]],
          merges: [],
          rich: defaultRichProjection(),
        },
      ],
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
  };
}

function recentFile(partial: Partial<RecentFile> = {}): RecentFile {
  return {
    id: "recent-1",
    path: "/tmp/recent.xlsx",
    fileName: "recent.xlsx",
    lastOpened: 1,
    fileSize: 42,
    storageType: "desktopPath",
    ...partial,
  };
}

async function flushPromises() {
  for (let i = 0; i < 8; i += 1) {
    await Promise.resolve();
  }
}

describe("useHomeFileActions", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    spreadsheetFormats.defaultSpreadsheetExtension.mockResolvedValue("xlsx");
  });

  it("creates a new workbook through the document session boundary", async () => {
    const api = await import("@/api");
    const documentSessionStore = useDocumentSessionStore();
    const navigateToTable = vi.fn();
    vi.mocked(api.initFile).mockResolvedValue(openedResponse("untitled.xlsx", 1));

    const actions = useHomeFileActions({ navigateToTable });

    await actions.handleNewFile();

    expect(api.initFile).toHaveBeenCalledTimes(1);
    expect(documentSessionStore.documentId).toBe(1);
    expect(documentSessionStore.data?.fileName).toBe("untitled.xlsx");
    expect(navigateToTable).toHaveBeenCalledTimes(1);
  });

  it("keeps the previous document when new workbook initialization fails", async () => {
    const api = await import("@/api");
    const documentSessionStore = useDocumentSessionStore();
    documentSessionStore.openDocumentResponse(openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    vi.mocked(api.initFile).mockRejectedValue(new Error("init failed"));

    const actions = useHomeFileActions({ navigateToTable: vi.fn() });

    await actions.handleNewFile();

    expect(documentSessionStore.documentId).toBe(1);
    expect(documentSessionStore.currentFilePath).toBe("/tmp/current.xlsx");
    expect(documentSessionStore.data?.fileName).toBe("current.xlsx");
  });

  it("resumes pending autosave when default format lookup fails during new workbook creation", async () => {
    const unsavedChanges = await import("@/utils/unsavedChanges");
    vi.useFakeTimers();
    const statusStore = useDocumentStatusStore();
    const pendingStore = usePendingCellSavesStore();
    const committed: string[] = [];
    statusStore.markPendingContentChange();
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
    spreadsheetFormats.defaultSpreadsheetExtension.mockRejectedValue(
      new Error("format unavailable")
    );
    vi.mocked(unsavedChanges.confirmDiscardUnsavedChanges).mockResolvedValue(true);

    try {
      const actions = useHomeFileActions({ navigateToTable: vi.fn() });

      await actions.handleNewFile();
      await vi.advanceTimersByTimeAsync(100);

      expect(spreadsheetFormats.defaultSpreadsheetExtension).toHaveBeenCalledTimes(1);
      expect(committed).toEqual(["draft"]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("disables home file actions while the document session is locked", async () => {
    const api = await import("@/api");
    const documentSessionStore = useDocumentSessionStore();
    documentSessionStore.beginLifecycle("saving");

    const actions = useHomeFileActions({ navigateToTable: vi.fn() });

    expect(actions.isBusy.value).toBe(true);
    await actions.handleNewFile();

    expect(api.initFile).not.toHaveBeenCalled();
  });

  it("opens an existing recent file from the home workflow", async () => {
    const api = await import("@/api");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const navigateToTable = vi.fn();
    vi.mocked(api.checkFileExists).mockResolvedValue(true);
    vi.mocked(platform.readFile).mockResolvedValue(openedResponse("recent.xlsx", 2));

    const actions = useHomeFileActions({ navigateToTable });

    await actions.handleOpenRecent(recentFile());
    await flushPromises();

    expect(platform.readFile).toHaveBeenCalledWith("/tmp/recent.xlsx");
    expect(documentSessionStore.documentId).toBe(2);
    expect(documentSessionStore.currentFilePath).toBe("/tmp/recent.xlsx");
    expect(navigateToTable).toHaveBeenCalledTimes(1);
  });
});
