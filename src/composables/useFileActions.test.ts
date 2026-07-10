import { beforeEach, describe, expect, it, vi } from "vitest";
import { computed } from "vue";
import { createPinia, setActivePinia } from "pinia";
import { useFileActions } from "@/composables/useFileActions";
import { useDocumentSessionStore } from "@/stores/documentSession";
import { useDocumentStatusStore } from "@/stores/documentStatus";
import { usePendingCellSavesStore } from "@/stores/pendingCellSaves";
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

const routerMocks = vi.hoisted(() => ({
  push: vi.fn(),
}));

vi.mock("vue-router", () => ({
  useRouter: () => ({
    push: routerMocks.push,
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
  getSpreadsheetFormatOptions: vi.fn().mockResolvedValue({
    defaultExtension: "xlsx",
    supportedExtensions: ["xlsx", "csv"],
  }),
  getRecentFiles: vi.fn().mockResolvedValue([]),
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

async function waitForCondition(condition: () => boolean) {
  for (let i = 0; i < 32; i += 1) {
    if (condition()) return;
    await Promise.resolve();
  }
  throw new Error("Timed out waiting for condition");
}

function mountActions(flushPendingCellChanges: () => Promise<boolean>) {
  const documentSessionStore = useDocumentSessionStore();
  return useFileActions({
    fileData: computed(() => documentSessionStore.data),
    flushPendingCellChanges,
  });
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

  it("drops a path load that is cancelled while reading", async () => {
    const api = await import("@/api");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const pendingRead = deferred<OpenDocumentResponse>();
    let shouldContinue = true;
    vi.mocked(platform.readFile).mockReturnValue(pendingRead.promise);

    const actions = mountActions(vi.fn().mockResolvedValue(true));
    const loadPromise = actions.loadFileFromPath(
      "/tmp/stale.xlsx",
      () => shouldContinue
    );

    await flushPromises();
    expect(platform.readFile).toHaveBeenCalledWith("/tmp/stale.xlsx");

    shouldContinue = false;
    pendingRead.resolve(openedResponse("stale.xlsx", 2));

    await expect(loadPromise).resolves.toBe(false);
    expect(documentSessionStore.documentId).toBeNull();
    expect(documentSessionStore.currentFilePath).toBeNull();
    expect(api.addRecentFileWithThumbnail).not.toHaveBeenCalled();
  });

  it("releases the loading lifecycle when an in-flight path load is cancelled", async () => {
    const api = await import("@/api");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const pendingRead = deferred<OpenDocumentResponse>();
    const cancelHandlers: Array<() => void> = [];
    let shouldContinue = true;
    const guard = (() => shouldContinue) as (() => boolean) & {
      onCancel: (handler: () => void) => () => void;
    };
    guard.onCancel = (handler) => {
      cancelHandlers.push(handler);
      return () => undefined;
    };
    vi.mocked(platform.readFile).mockReturnValue(pendingRead.promise);

    const actions = mountActions(vi.fn().mockResolvedValue(true));
    const loadPromise = actions.loadFileFromPath("/tmp/slow.xlsx", guard);

    await flushPromises();

    expect(documentSessionStore.lifecycle).toBe("loading");
    expect(documentSessionStore.isInteractionLocked).toBe(true);

    shouldContinue = false;
    for (const handler of cancelHandlers) {
      handler();
    }

    expect(documentSessionStore.lifecycle).toBe("idle");
    expect(documentSessionStore.isInteractionLocked).toBe(false);

    pendingRead.resolve(openedResponse("slow.xlsx", 2));
    await expect(loadPromise).resolves.toBe(false);
    expect(documentSessionStore.documentId).toBeNull();
    expect(documentSessionStore.currentFilePath).toBeNull();
    expect(api.addRecentFileWithThumbnail).not.toHaveBeenCalled();
  });

  it("suppresses stale path load errors after cancellation", async () => {
    const elementPlus = await import("element-plus");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const pendingRead = deferred<OpenDocumentResponse>();
    const cancelHandlers: Array<() => void> = [];
    let shouldContinue = true;
    const guard = (() => shouldContinue) as (() => boolean) & {
      onCancel: (handler: () => void) => () => void;
    };
    guard.onCancel = (handler) => {
      cancelHandlers.push(handler);
      return () => undefined;
    };
    vi.mocked(platform.readFile).mockReturnValue(pendingRead.promise);

    const actions = mountActions(vi.fn().mockResolvedValue(true));
    const loadPromise = actions.loadFileFromPath("/tmp/stale-error.xlsx", guard);

    await flushPromises();

    shouldContinue = false;
    for (const handler of cancelHandlers) {
      handler();
    }
    pendingRead.reject(new Error("stale read failed"));

    await expect(loadPromise).resolves.toBe(false);
    expect(elementPlus.ElMessage.error).not.toHaveBeenCalled();
    expect(documentSessionStore.data).toBeNull();
    expect(documentSessionStore.lifecycle).toBe("idle");
  });

  it("waits for an active document lifecycle before loading a route file path", async () => {
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    documentSessionStore.beginLifecycle("saving");
    vi.mocked(platform.readFile).mockResolvedValue(openedResponse("queued.xlsx", 2));

    const actions = mountActions(flushPendingCellChanges);
    const loadPromise = actions.loadFileFromPath("/tmp/queued.xlsx");

    await flushPromises();

    expect(platform.readFile).not.toHaveBeenCalled();
    expect(documentSessionStore.lifecycle).toBe("saving");

    documentSessionStore.endLifecycle("saving");

    await expect(loadPromise).resolves.toBe(true);
    expect(platform.readFile).toHaveBeenCalledWith("/tmp/queued.xlsx");
    expect(documentSessionStore.currentFilePath).toBe("/tmp/queued.xlsx");
  });

  it("waits for an active editor command before loading a route file path", async () => {
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    const releaseEditorCommand = documentSessionStore.beginEditorCommand();
    vi.mocked(platform.readFile).mockResolvedValue(openedResponse("queued.xlsx", 2));

    const actions = mountActions(flushPendingCellChanges);
    const loadPromise = actions.loadFileFromPath("/tmp/queued.xlsx");

    await flushPromises();

    expect(platform.readFile).not.toHaveBeenCalled();
    expect(documentSessionStore.lifecycle).toBe("idle");
    expect(documentSessionStore.isInteractionLocked).toBe(true);

    releaseEditorCommand?.();

    await expect(loadPromise).resolves.toBe(true);
    expect(platform.readFile).toHaveBeenCalledWith("/tmp/queued.xlsx");
    expect(documentSessionStore.currentFilePath).toBe("/tmp/queued.xlsx");
  });

  it("does not block path loading on recent file metadata updates", async () => {
    const api = await import("@/api");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const recentUpdate = deferred<Awaited<ReturnType<typeof api.addRecentFileWithThumbnail>>>();
    vi.mocked(api.addRecentFileWithThumbnail).mockReturnValue(recentUpdate.promise);
    vi.mocked(platform.readFile).mockResolvedValue(openedResponse("fast.xlsx", 2));

    const actions = mountActions(vi.fn().mockResolvedValue(true));

    await expect(actions.loadFileFromPath("/tmp/fast.xlsx")).resolves.toBe(true);

    expect(documentSessionStore.currentFilePath).toBe("/tmp/fast.xlsx");
    expect(api.addRecentFileWithThumbnail).toHaveBeenCalledWith(
      { documentId: 2, baseRevision: 0 },
      undefined
    );

    recentUpdate.resolve({
      id: "recent",
      path: "/tmp/fast.xlsx",
      fileName: "fast.xlsx",
      lastOpened: 1,
      fileSize: 42,
      storageType: "desktopPath",
    });
    await flushPromises();
  });

  it("keeps a route-loaded document open when recent metadata refresh fails", async () => {
    const api = await import("@/api");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.mocked(platform.readFile).mockResolvedValue(openedResponse("nameless.xlsx", 2));
    vi.mocked(api.addRecentFileWithThumbnail).mockRejectedValueOnce(
      new Error("metadata unavailable")
    );

    try {
      const actions = mountActions(vi.fn().mockResolvedValue(true));

      await expect(actions.loadFileFromPath("/tmp/nameless.xlsx")).resolves.toBe(true);
      await flushPromises();

      expect(documentSessionStore.documentId).toBe(2);
      expect(documentSessionStore.currentFilePath).toBe("/tmp/nameless.xlsx");
      expect(api.addRecentFileWithThumbnail).toHaveBeenCalledWith(
        { documentId: 2, baseRevision: 0 },
        undefined
      );
      expect(warn).toHaveBeenCalled();
    } finally {
      warn.mockRestore();
    }
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
    const unsavedChanges = await import("@/utils/unsavedChanges");
    const documentSessionStore = useDocumentSessionStore();
    const statusStore = useDocumentStatusStore();
    const pendingCellSavesStore = usePendingCellSavesStore();
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    const selection = {
      path: "/tmp/broken.xlsx",
      fileName: "broken.xlsx",
      originalPath: "content://broken",
    };
    documentSessionStore.openDocumentResponse(openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    statusStore.markPendingContentChange();
    pendingCellSavesStore.applyDraft(
      "0,0,0",
      { sheetIndex: 0, row: 0, col: 0, value: "draft", oldValue: text("current") },
      text("current")
    );
    vi.mocked(unsavedChanges.confirmDiscardUnsavedChanges).mockResolvedValue(true);
    vi.mocked(platform.pickOpenFile).mockResolvedValue(selection);
    vi.mocked(platform.readFile).mockRejectedValue(new Error("cannot parse"));

    const actions = mountActions(flushPendingCellChanges);

    await actions.handleOpenFile();

    expect(platform.readFile).toHaveBeenCalledWith("/tmp/broken.xlsx");
    expect(unsavedChanges.confirmDiscardUnsavedChanges).toHaveBeenCalledTimes(1);
    expect(flushPendingCellChanges).not.toHaveBeenCalled();
    expect(platform.discardOpenFileSelection).toHaveBeenCalledWith(selection);
    expect(documentSessionStore.currentFilePath).toBe("/tmp/current.xlsx");
    expect(statusStore.hasPendingContentChange).toBe(true);
    expect(pendingCellSavesStore.draftCellValues.get("0,0,0")).toBe("draft");
  });

  it("allows closing a stale projection after discard confirmation", async () => {
    const api = await import("@/api");
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
    documentSessionStore.projectionStale = true;
    vi.mocked(unsavedChanges.confirmDiscardUnsavedChanges).mockResolvedValue(true);

    const actions = mountActions(flushPendingCellChanges);

    await expect(actions.closeCurrentDocument()).resolves.toBe(true);

    expect(api.closeCurrentDocument).toHaveBeenCalledWith(1);
    expect(flushPendingCellChanges).not.toHaveBeenCalled();
    expect(documentSessionStore.data).toBeNull();
    expect(documentSessionStore.documentId).toBeNull();
    expect(documentSessionStore.projectionStale).toBe(false);
  });

  it("locks document interaction while backend close is pending", async () => {
    const api = await import("@/api");
    const documentSessionStore = useDocumentSessionStore();
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    const pendingClose = deferred<void>();
    documentSessionStore.openDocumentResponse(openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    vi.mocked(api.closeCurrentDocument).mockReturnValue(pendingClose.promise);

    const actions = mountActions(flushPendingCellChanges);
    const closePromise = actions.closeCurrentDocument();

    await flushPromises();

    expect(documentSessionStore.lifecycle).toBe("closing");
    expect(documentSessionStore.isInteractionLocked).toBe(true);

    await expect(actions.closeCurrentDocument()).resolves.toBe(false);
    expect(api.closeCurrentDocument).toHaveBeenCalledTimes(1);

    pendingClose.resolve();

    await expect(closePromise).resolves.toBe(true);
    expect(documentSessionStore.lifecycle).toBe("idle");
    expect(documentSessionStore.data).toBeNull();
  });

  it("delegates document closing to the route-leave guard when returning home", async () => {
    const api = await import("@/api");
    const documentSessionStore = useDocumentSessionStore();
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    const pendingNavigation = deferred<void>();
    let backResolved = false;
    documentSessionStore.openDocumentResponse(openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    routerMocks.push.mockReturnValue(pendingNavigation.promise);

    const actions = mountActions(flushPendingCellChanges);
    const backPromise = actions.handleBack().then(() => {
      backResolved = true;
    });

    await waitForCondition(() => routerMocks.push.mock.calls.length > 0);

    expect(api.closeCurrentDocument).not.toHaveBeenCalled();
    expect(routerMocks.push).toHaveBeenCalledWith({ name: "home" });
    expect(documentSessionStore.documentId).toBe(1);
    expect(backResolved).toBe(false);

    pendingNavigation.resolve();
    await backPromise;

    expect(backResolved).toBe(true);
  });

  it("reports a home navigation failure after closing the current document", async () => {
    const api = await import("@/api");
    const elementPlus = await import("element-plus");
    const documentSessionStore = useDocumentSessionStore();
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    documentSessionStore.openDocumentResponse(openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    routerMocks.push.mockRejectedValue(new Error("navigation failed"));

    const actions = mountActions(flushPendingCellChanges);

    await expect(actions.handleBack()).resolves.toBeUndefined();

    expect(api.closeCurrentDocument).not.toHaveBeenCalled();
    expect(routerMocks.push).toHaveBeenCalledWith({ name: "home" });
    expect(documentSessionStore.documentId).toBe(1);
    expect(elementPlus.ElMessage.error).toHaveBeenCalledWith(
      "Failed to return home: Error: navigation failed"
    );
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

  it("keeps a save-as result when recent metadata refresh fails", async () => {
    const api = await import("@/api");
    const elementPlus = await import("element-plus");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const savePath = "/tmp/saved-without-name.xlsx";
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    documentSessionStore.openDocumentResponse(newUntitledResponse(1), null);
    vi.mocked(api.getNativeSavePlan)
      .mockResolvedValueOnce(nativeSavePlan({ requiresSaveAs: true }))
      .mockResolvedValueOnce(nativeSavePlan());
    vi.mocked(platform.pickSaveLocation).mockResolvedValue(savePath);
    vi.mocked(platform.saveFile).mockResolvedValue(savedResponse("saved-without-name.xlsx", savePath, 1));
    vi.mocked(api.addRecentFileWithThumbnail).mockRejectedValueOnce(
      new Error("metadata unavailable")
    );

    try {
      const actions = mountActions(flushPendingCellChanges);

      await actions.handleSaveFile();
      await flushPromises();

      expect(platform.saveFile).toHaveBeenCalledWith(savePath, {
        documentId: 1,
        baseRevision: 0,
      });
      expect(platform.discardSaveLocation).not.toHaveBeenCalled();
      expect(documentSessionStore.currentFilePath).toBe(savePath);
      expect(api.addRecentFileWithThumbnail).toHaveBeenCalledWith(
        { documentId: 1, baseRevision: 0 },
        undefined
      );
      expect(warn).toHaveBeenCalled();
      expect(elementPlus.ElMessage.error).not.toHaveBeenCalled();
      expect(elementPlus.ElMessage.success).toHaveBeenCalledWith("File saved successfully");
    } finally {
      warn.mockRestore();
    }
  });

  it("keeps an existing-file save when recent metadata refresh fails", async () => {
    const api = await import("@/api");
    const elementPlus = await import("element-plus");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const existingPath = "/tmp/current.xlsx";
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    documentSessionStore.openDocumentResponse(openedResponse("current.xlsx", 1), existingPath);
    vi.mocked(api.getNativeSavePlan).mockResolvedValueOnce(nativeSavePlan());
    vi.mocked(platform.saveFile).mockResolvedValue(savedResponse("current.xlsx", existingPath, 1));
    vi.mocked(api.addRecentFileWithThumbnail).mockRejectedValueOnce(
      new Error("metadata unavailable")
    );

    try {
      const actions = mountActions(flushPendingCellChanges);

      await actions.handleSaveFile();
      await flushPromises();

      expect(platform.saveFile).toHaveBeenCalledWith(existingPath, {
        documentId: 1,
        baseRevision: 0,
      });
      expect(documentSessionStore.currentFilePath).toBe(existingPath);
      expect(api.addRecentFileWithThumbnail).toHaveBeenCalledWith(
        { documentId: 1, baseRevision: 0 },
        undefined
      );
      expect(warn).toHaveBeenCalled();
      expect(elementPlus.ElMessage.error).not.toHaveBeenCalled();
      expect(elementPlus.ElMessage.success).toHaveBeenCalledWith("File saved successfully");
    } finally {
      warn.mockRestore();
    }
  });

  it("allows saving a stale projection and replaces it with the saved backend snapshot", async () => {
    const api = await import("@/api");
    const elementPlus = await import("element-plus");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const existingPath = "/tmp/current.xlsx";
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    documentSessionStore.openDocumentResponse(openedResponse("current.xlsx", 1), existingPath);
    documentSessionStore.revision = 3;
    documentSessionStore.projectionStale = true;
    vi.mocked(api.getNativeSavePlan).mockResolvedValueOnce(nativeSavePlan());
    const saved = savedResponse("current.xlsx", existingPath, 1);
    vi.mocked(platform.saveFile).mockResolvedValue({
      ...saved,
      editorSession: {
        ...saved.editorSession,
        revision: 3,
      },
    });

    const actions = mountActions(flushPendingCellChanges);

    await actions.handleSaveFile();

    expect(platform.saveFile).toHaveBeenCalledWith(existingPath, {
      documentId: 1,
      baseRevision: 3,
    });
    expect(documentSessionStore.projectionStale).toBe(false);
    expect(documentSessionStore.data?.sheets[0].rows[0][0]).toEqual(text("saved"));
    expect(elementPlus.ElMessage.success).toHaveBeenCalledWith("File saved successfully");
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
