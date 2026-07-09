import { beforeEach, describe, expect, it, vi } from "vitest";
import { computed, ref } from "vue";
import { createPinia, setActivePinia } from "pinia";
import { useFileActions } from "@/composables/useFileActions";
import { useDocumentSessionStore } from "@/stores/documentSession";
import { useDocumentStatusStore } from "@/stores/documentStatus";
import {
  defaultHistoryStatus,
  defaultRichProjection,
  defaultWorkbookCapabilities,
  readyFormulaStatus,
  type CellValue,
  type FileData,
  type OpenDocumentResponse,
  type SheetData,
} from "@/types";

vi.mock("vue-router", () => ({
  useRouter: () => ({
    push: vi.fn(),
  }),
}));

vi.mock("element-plus", () => ({
  ElMessage: {
    error: vi.fn(),
    success: vi.fn(),
    warning: vi.fn(),
  },
}));

vi.mock("@/api", () => ({
  getFileSize: vi.fn().mockResolvedValue(42),
  getSpreadsheetFormatOptions: vi.fn().mockResolvedValue({
    defaultExtension: "xlsx",
    openExtensions: ["xlsx", "csv"],
    saveExtensions: ["xlsx", "csv"],
    exportExtensions: ["xlsx", "csv"],
  }),
  addRecentFileWithThumbnail: vi.fn().mockResolvedValue({
    id: "recent",
    path: "/tmp/next.xlsx",
    fileName: "next.xlsx",
    lastOpened: 1,
    fileSize: 42,
    storageType: "desktopPath",
  }),
  closeCurrentDocument: vi.fn(),
}));

vi.mock("@/platform", () => ({
  exportFile: vi.fn(),
  getFileName: vi.fn(),
  getStorageType: vi.fn().mockResolvedValue("desktopPath"),
  pickOpenFile: vi.fn(),
  pickSaveLocation: vi.fn(),
  readFile: vi.fn(),
  saveFile: vi.fn(),
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

function text(value: string): CellValue {
  return { type: "cell", kind: "text", raw: value, display: value };
}

function sheet(name: string, rows: CellValue[][]): SheetData {
  return { name, rows, merges: [], rich: defaultRichProjection() };
}

function fileData(fileName: string, value: string): FileData {
  return {
    path: `/tmp/${fileName}`,
    fileName,
    sheets: [sheet("Sheet1", [[text(value)]])],
  };
}

function openedResponse(fileName: string, documentId: number): OpenDocumentResponse {
  return {
    fileData: fileData(fileName, "opened"),
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

function mountActions(flushPendingCellChanges: () => Promise<boolean>) {
  const documentSessionStore = useDocumentSessionStore();
  const currentSheetIndex = ref(0);
  const isLoading = ref(false);
  const isFileLoading = ref(false);
  const actions = useFileActions({
    fileData: computed(() => documentSessionStore.data),
    currentSheetIndex,
    isLoading,
    isFileLoading,
    flushPendingCellChanges,
  });
  return {
    ...actions,
    currentSheetIndex,
    isLoading,
    isFileLoading,
  };
}

describe("useFileActions", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("does not ask to discard when file picking is cancelled", async () => {
    const platform = await import("@/platform");
    const unsavedChanges = await import("@/utils/unsavedChanges");
    const documentSessionStore = useDocumentSessionStore();
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    documentSessionStore.openDocumentResponse(openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    vi.mocked(platform.pickOpenFile).mockResolvedValue(null);

    const actions = mountActions(flushPendingCellChanges);

    await actions.handleOpenFile();

    expect(unsavedChanges.confirmDiscardUnsavedChanges).not.toHaveBeenCalled();
    expect(flushPendingCellChanges).not.toHaveBeenCalled();
    expect(platform.readFile).not.toHaveBeenCalled();
    expect(documentSessionStore.currentFilePath).toBe("/tmp/current.xlsx");
  });

  it("asks before opening when pending work becomes dirty after flush", async () => {
    const platform = await import("@/platform");
    const unsavedChanges = await import("@/utils/unsavedChanges");
    const documentSessionStore = useDocumentSessionStore();
    const statusStore = useDocumentStatusStore();
    documentSessionStore.openDocumentResponse(openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    vi.mocked(unsavedChanges.confirmDiscardUnsavedChanges).mockResolvedValue(false);
    vi.mocked(platform.pickOpenFile).mockResolvedValue({
      path: "/tmp/next.xlsx",
      fileName: "next.xlsx",
    });

    const actions = mountActions(async () => {
      statusStore.applyEditorState({
        canUndo: true,
        canRedo: false,
        isDirty: true,
        history: defaultHistoryStatus(),
      });
      return true;
    });

    await actions.handleOpenFile();

    expect(unsavedChanges.confirmDiscardUnsavedChanges).toHaveBeenCalledTimes(1);
    expect(platform.readFile).not.toHaveBeenCalled();
    expect(documentSessionStore.currentFilePath).toBe("/tmp/current.xlsx");
  });

  it("does not flush discarded work when the current document is already dirty", async () => {
    const platform = await import("@/platform");
    const unsavedChanges = await import("@/utils/unsavedChanges");
    const documentSessionStore = useDocumentSessionStore();
    const statusStore = useDocumentStatusStore();
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    documentSessionStore.openDocumentResponse(openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    statusStore.applyEditorState({
      canUndo: true,
      canRedo: false,
      isDirty: true,
      history: defaultHistoryStatus(),
    });
    vi.mocked(unsavedChanges.confirmDiscardUnsavedChanges).mockResolvedValue(true);
    vi.mocked(platform.pickOpenFile).mockResolvedValue({
      path: "/tmp/next.xlsx",
      fileName: "next.xlsx",
    });
    vi.mocked(platform.readFile).mockResolvedValue(openedResponse("next.xlsx", 2));

    const actions = mountActions(flushPendingCellChanges);

    await actions.handleOpenFile();

    expect(unsavedChanges.confirmDiscardUnsavedChanges).toHaveBeenCalledTimes(1);
    expect(flushPendingCellChanges).not.toHaveBeenCalled();
    expect(platform.pickOpenFile).toHaveBeenCalledTimes(1);
    expect(platform.readFile).toHaveBeenCalledWith("/tmp/next.xlsx");
    expect(documentSessionStore.documentId).toBe(2);
  });

  it("discards newly dirty pending work before opening the selected file", async () => {
    const platform = await import("@/platform");
    const unsavedChanges = await import("@/utils/unsavedChanges");
    const documentSessionStore = useDocumentSessionStore();
    const statusStore = useDocumentStatusStore();
    documentSessionStore.openDocumentResponse(openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    vi.mocked(unsavedChanges.confirmDiscardUnsavedChanges).mockResolvedValue(true);
    vi.mocked(platform.pickOpenFile).mockResolvedValue({
      path: "/tmp/next.xlsx",
      fileName: "next.xlsx",
    });
    vi.mocked(platform.readFile).mockResolvedValue(openedResponse("next.xlsx", 2));

    const actions = mountActions(async () => {
      statusStore.applyEditorState({
        canUndo: true,
        canRedo: false,
        isDirty: true,
        history: defaultHistoryStatus(),
      });
      return true;
    });

    await actions.handleOpenFile();

    expect(unsavedChanges.confirmDiscardUnsavedChanges).toHaveBeenCalledTimes(1);
    expect(platform.pickOpenFile).toHaveBeenCalledTimes(1);
    expect(platform.readFile).toHaveBeenCalledWith("/tmp/next.xlsx");
    expect(documentSessionStore.documentId).toBe(2);
    expect(documentSessionStore.currentFilePath).toBe("/tmp/next.xlsx");
  });
});
