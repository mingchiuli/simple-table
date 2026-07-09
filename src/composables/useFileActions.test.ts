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
  type DocumentCapabilities,
  type FileData,
  type NativeSavePlan,
  type OpenDocumentResponse,
  type SavedDocumentResponse,
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
  getNativeSavePlan: vi.fn(),
}));

vi.mock("@/platform", () => ({
  discardOpenFileSelection: vi.fn(),
  discardSaveLocation: vi.fn(),
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

function newUntitledResponse(documentId: number): OpenDocumentResponse {
  return {
    ...openedResponse("untitled.xlsx", documentId),
    fileData: {
      ...fileData("untitled.xlsx", "opened"),
      path: "",
    },
  };
}

function savedResponse(fileName: string, path: string, documentId: number): SavedDocumentResponse {
  return {
    ...openedResponse(fileName, documentId),
    fileData: {
      ...fileData(fileName, "saved"),
      path,
    },
  };
}

function documentCapabilities(): DocumentCapabilities {
  return {
    sourceFormat: "xlsx",
    canSaveOriginal: true,
    nativeSaveFormat: "xlsx",
    exportFormats: ["xlsx", "csv"],
    nativeSaveExtension: "xlsx",
    exportExtension: "xlsx",
    requiresSaveAsForNativeSave: false,
    workbook: defaultWorkbookCapabilities(),
  };
}

function nativeSavePlan(partial: Partial<NativeSavePlan> = {}): NativeSavePlan {
  return {
    canSave: true,
    requiresSaveAs: false,
    nativeSaveExtension: "xlsx",
    defaultExtension: "xlsx",
    blockedReason: null,
    capabilities: documentCapabilities(),
    ...partial,
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
    expect(platform.discardOpenFileSelection).not.toHaveBeenCalled();
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
    expect(platform.discardOpenFileSelection).toHaveBeenCalledWith({
      path: "/tmp/next.xlsx",
      fileName: "next.xlsx",
    });
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
    expect(platform.discardOpenFileSelection).not.toHaveBeenCalled();
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
    expect(platform.discardOpenFileSelection).not.toHaveBeenCalled();
    expect(platform.readFile).toHaveBeenCalledWith("/tmp/next.xlsx");
    expect(documentSessionStore.documentId).toBe(2);
    expect(documentSessionStore.currentFilePath).toBe("/tmp/next.xlsx");
  });

  it("discards selected imported files when reading fails", async () => {
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    const selection = {
      path: "/tmp/broken.xlsx",
      fileName: "broken.xlsx",
      originalPath: "content://broken",
    };
    documentSessionStore.openDocumentResponse(openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    vi.mocked(platform.pickOpenFile).mockResolvedValue(selection);
    vi.mocked(platform.readFile).mockRejectedValue(new Error("cannot parse"));

    const actions = mountActions(flushPendingCellChanges);

    await actions.handleOpenFile();

    expect(platform.readFile).toHaveBeenCalledWith("/tmp/broken.xlsx");
    expect(platform.discardOpenFileSelection).toHaveBeenCalledWith(selection);
    expect(documentSessionStore.currentFilePath).toBe("/tmp/current.xlsx");
  });

  it("discards a reserved save-as location when the target cannot be saved", async () => {
    const api = await import("@/api");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const savePath = "/tmp/reserved.xlsx";
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    documentSessionStore.openDocumentResponse(newUntitledResponse(1), null);
    vi.mocked(api.getNativeSavePlan)
      .mockResolvedValueOnce(nativeSavePlan({ requiresSaveAs: true }))
      .mockResolvedValueOnce(nativeSavePlan({
        canSave: false,
        blockedReason: "blocked",
      }));
    vi.mocked(platform.pickSaveLocation).mockResolvedValue(savePath);

    const actions = mountActions(flushPendingCellChanges);

    await actions.handleSaveFile();

    expect(platform.saveFile).not.toHaveBeenCalled();
    expect(platform.discardSaveLocation).toHaveBeenCalledWith(savePath);
    expect(documentSessionStore.currentFilePath).toBeNull();
  });

  it("keeps a reserved save-as location after successful save", async () => {
    const api = await import("@/api");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const savePath = "/tmp/saved.xlsx";
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    documentSessionStore.openDocumentResponse(newUntitledResponse(1), null);
    vi.mocked(api.getNativeSavePlan)
      .mockResolvedValueOnce(nativeSavePlan({ requiresSaveAs: true }))
      .mockResolvedValueOnce(nativeSavePlan());
    vi.mocked(platform.pickSaveLocation).mockResolvedValue(savePath);
    vi.mocked(platform.saveFile).mockResolvedValue(savedResponse("saved.xlsx", savePath, 1));

    const actions = mountActions(flushPendingCellChanges);

    await actions.handleSaveFile();

    expect(platform.saveFile).toHaveBeenCalledWith(savePath, {
      documentId: 1,
      baseRevision: 0,
    });
    expect(platform.discardSaveLocation).not.toHaveBeenCalled();
    expect(documentSessionStore.currentFilePath).toBe(savePath);
  });

  it("keeps a reserved save-as location when a successful save response is ignored as stale", async () => {
    const api = await import("@/api");
    const elementPlus = await import("element-plus");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const savePath = "/tmp/stale-response.xlsx";
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    documentSessionStore.openDocumentResponse(newUntitledResponse(1), null);
    vi.mocked(api.getNativeSavePlan)
      .mockResolvedValueOnce(nativeSavePlan({ requiresSaveAs: true }))
      .mockResolvedValueOnce(nativeSavePlan());
    vi.mocked(platform.pickSaveLocation).mockResolvedValue(savePath);
    vi.mocked(platform.saveFile).mockResolvedValue(savedResponse("stale-response.xlsx", savePath, 2));

    const actions = mountActions(flushPendingCellChanges);

    await actions.handleSaveFile();

    expect(platform.saveFile).toHaveBeenCalledWith(savePath, {
      documentId: 1,
      baseRevision: 0,
    });
    expect(platform.discardSaveLocation).not.toHaveBeenCalled();
    expect(elementPlus.ElMessage.warning).toHaveBeenCalledWith(
      "File was saved, but the active document changed before the editor could refresh."
    );
    expect(elementPlus.ElMessage.success).not.toHaveBeenCalled();
    expect(api.addRecentFileWithThumbnail).not.toHaveBeenCalled();
    expect(documentSessionStore.documentId).toBe(1);
    expect(documentSessionStore.currentFilePath).toBeNull();
  });

  it("warns when an existing-file save succeeds but its response is ignored as stale", async () => {
    const api = await import("@/api");
    const elementPlus = await import("element-plus");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const existingPath = "/tmp/current.xlsx";
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    documentSessionStore.openDocumentResponse(openedResponse("current.xlsx", 1), existingPath);
    vi.mocked(api.getNativeSavePlan).mockResolvedValueOnce(nativeSavePlan());
    vi.mocked(platform.saveFile).mockResolvedValue(savedResponse("current.xlsx", existingPath, 2));

    const actions = mountActions(flushPendingCellChanges);

    await actions.handleSaveFile();

    expect(platform.saveFile).toHaveBeenCalledWith(existingPath, {
      documentId: 1,
      baseRevision: 0,
    });
    expect(elementPlus.ElMessage.warning).toHaveBeenCalledWith(
      "File was saved, but the active document changed before the editor could refresh."
    );
    expect(elementPlus.ElMessage.success).not.toHaveBeenCalled();
    expect(api.addRecentFileWithThumbnail).not.toHaveBeenCalled();
    expect(documentSessionStore.currentFilePath).toBe(existingPath);
  });

  it("discards a reserved save-as location when writing fails", async () => {
    const api = await import("@/api");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const savePath = "/tmp/write-failed.xlsx";
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    documentSessionStore.openDocumentResponse(newUntitledResponse(1), null);
    vi.mocked(api.getNativeSavePlan)
      .mockResolvedValueOnce(nativeSavePlan({ requiresSaveAs: true }))
      .mockResolvedValueOnce(nativeSavePlan());
    vi.mocked(platform.pickSaveLocation).mockResolvedValue(savePath);
    vi.mocked(platform.saveFile).mockRejectedValue(new Error("disk full"));

    const actions = mountActions(flushPendingCellChanges);

    await actions.handleSaveFile();

    expect(platform.saveFile).toHaveBeenCalledWith(savePath, {
      documentId: 1,
      baseRevision: 0,
    });
    expect(platform.discardSaveLocation).toHaveBeenCalledWith(savePath);
    expect(documentSessionStore.currentFilePath).toBeNull();
  });
});
