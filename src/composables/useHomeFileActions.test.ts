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
import { openResponseFromFileData } from "@/test/documentFixtures";

const spreadsheetFormats = vi.hoisted(() => ({
  defaultSpreadsheetExtension: vi.fn(),
}));

const openProtocolMocks = vi.hoisted(() => ({
  prepareNewFile: vi.fn(),
  prepareOpenFile: vi.fn(),
  prepareRecentFile: vi.fn(),
  commitPreparedDocument: vi.fn(),
  abortPreparedDocument: vi.fn(),
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
  getSpreadsheetFormatOptions: vi.fn().mockResolvedValue({
    defaultExtension: "xlsx",
    supportedExtensions: ["xlsx", "csv"],
  }),
  prepareNewFile: openProtocolMocks.prepareNewFile,
  commitPreparedDocument: openProtocolMocks.commitPreparedDocument,
  abortPreparedDocument: openProtocolMocks.abortPreparedDocument,
  removeRecentFile: vi.fn(),
}));

vi.mock("@/platform", () => ({
  discardOpenFileSelection: vi.fn(),
  pickOpenFile: vi.fn(),
  prepareOpenFile: openProtocolMocks.prepareOpenFile,
  prepareRecentFile: openProtocolMocks.prepareRecentFile,
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

function openedResponse(fileName = "book.xlsx", documentId: number | string = '1'): OpenDocumentResponse {
  const fileData = {
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
  return openResponseFromFileData(fileData, editorSession);
}

function mockPreparedNew(response: OpenDocumentResponse, token = "prepared-new") {
  openProtocolMocks.prepareNewFile.mockResolvedValue({ token });
  openProtocolMocks.commitPreparedDocument.mockResolvedValue(response);
}

function mockPreparedRecent(response: OpenDocumentResponse, token = "prepared-recent") {
  openProtocolMocks.prepareRecentFile.mockResolvedValue({ token });
  openProtocolMocks.commitPreparedDocument.mockResolvedValue(response);
}

function mockPreparedSelection(response: OpenDocumentResponse, token = "prepared-selection") {
  openProtocolMocks.prepareOpenFile.mockResolvedValue({ token });
  openProtocolMocks.commitPreparedDocument.mockResolvedValue(response);
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
    mockPreparedNew(openedResponse("untitled.xlsx", 1));

    const actions = useHomeFileActions({ navigateToTable });

    await actions.handleNewFile();

    expect(api.prepareNewFile).toHaveBeenCalledTimes(1);
    expect(api.commitPreparedDocument).toHaveBeenCalledWith("prepared-new", null);
    expect(documentSessionStore.documentId).toBe('1');
    expect(documentSessionStore.data?.fileName).toBe("untitled.xlsx");
    expect(navigateToTable).toHaveBeenCalledTimes(1);
  });

  it("keeps the previous document when new workbook initialization fails", async () => {
    const api = await import("@/api");
    const documentSessionStore = useDocumentSessionStore();
    documentSessionStore.openDocumentResponse(openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    vi.mocked(api.prepareNewFile).mockRejectedValue(new Error("init failed"));

    const actions = useHomeFileActions({ navigateToTable: vi.fn() });

    await actions.handleNewFile();

    expect(documentSessionStore.documentId).toBe('1');
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

    expect(api.prepareNewFile).not.toHaveBeenCalled();
  });

  it("opens an existing recent file from the home workflow", async () => {
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const navigateToTable = vi.fn();
    mockPreparedRecent(openedResponse("recent.xlsx", 2));

    const actions = useHomeFileActions({ navigateToTable });

    await actions.handleOpenRecent(recentFile());
    await flushPromises();

    expect(platform.prepareRecentFile).toHaveBeenCalledWith(recentFile());
    expect(documentSessionStore.documentId).toBe('2');
    expect(documentSessionStore.currentFilePath).toBe("/tmp/recent.xlsx");
    expect(navigateToTable).toHaveBeenCalledTimes(1);
  });

  it("relocates a recent file after direct open fails", async () => {
    const api = await import("@/api");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const navigateToTable = vi.fn();
    const stale = recentFile({ id: "stale", path: "/tmp/missing.xlsx" });
    vi.mocked(platform.prepareRecentFile).mockRejectedValueOnce(
      {
        code: "file_not_found",
        message: "File not found: /tmp/missing.xlsx",
      }
    );
    vi.mocked(platform.pickOpenFile).mockResolvedValue({
      path: "/tmp/relocated.xlsx",
      fileName: "relocated.xlsx",
    });
    mockPreparedSelection(openedResponse("relocated.xlsx", 3));

    const actions = useHomeFileActions({ navigateToTable });

    await actions.handleOpenRecent(stale);
    await flushPromises();

    expect(platform.prepareRecentFile).toHaveBeenCalledWith(stale);
    expect(platform.pickOpenFile).toHaveBeenCalledTimes(1);
    expect(api.removeRecentFile).toHaveBeenCalledWith("stale");
    expect(documentSessionStore.documentId).toBe('3');
    expect(documentSessionStore.currentFilePath).toBe("/tmp/relocated.xlsx");
    expect(navigateToTable).toHaveBeenCalledTimes(1);
  });

  it("reports recent file parse failures instead of relocating", async () => {
    const elementPlus = await import("element-plus");
    const platform = await import("@/platform");
    const navigateToTable = vi.fn();
    vi.mocked(platform.prepareRecentFile).mockRejectedValueOnce(
      new Error("Unsupported file format")
    );

    const actions = useHomeFileActions({ navigateToTable });

    await actions.handleOpenRecent(recentFile());
    await flushPromises();

    expect(platform.pickOpenFile).not.toHaveBeenCalled();
    expect(navigateToTable).not.toHaveBeenCalled();
    expect(elementPlus.ElMessage.error).toHaveBeenCalledWith(
      "Failed to open file: Error: Unsupported file format"
    );
  });
});
