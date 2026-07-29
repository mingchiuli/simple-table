import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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
  type NativeSavePlan,
  type PreparedOpenDocument,
} from "@/types";
import type { OpenDocumentResponse, SavedDocumentResponse } from '@/types/protocol';
import type { OperationCancellationSignal } from '@/application/operationCancellation';
import {
  openResponseFromFileData,
  preparedOpenDocument,
  savedResponseFromFileData,
  type FileData,
  type SheetData,
} from "@/test/documentFixtures";
import { openDocumentSession } from '@/test/documentSessionTestDriver';
import {
  createApplicationWorkspaceTestContext,
  type ApplicationWorkspaceTestContext,
} from '@/test/documentWorkspaceTestContext';

let workspace: ApplicationWorkspaceTestContext;

const routerMocks = vi.hoisted(() => ({
  push: vi.fn(),
}));

const openProtocolMocks = vi.hoisted(() => ({
  prepareOpenFile: vi.fn(),
  commitPreparedDocument: vi.fn(),
  abortPreparedDocument: vi.fn(),
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
  getDocumentCapabilities: vi.fn(),
  getRecentFiles: vi.fn().mockResolvedValue([]),
  removeRecentFile: vi.fn().mockResolvedValue(undefined),
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
  commitPreparedDocument: openProtocolMocks.commitPreparedDocument,
  getFileOperationResult: vi.fn().mockResolvedValue({ status: 'missing' }),
  getActiveDocument: vi.fn().mockResolvedValue(null),
  abortPreparedDocument: openProtocolMocks.abortPreparedDocument,
}));

vi.mock("@/platform", () => ({
  discardOpenFileSelection: vi.fn(),
  discardSaveLocation: vi.fn(),
  exportFile: vi.fn(),
  pickOpenFile: vi.fn(),
  pickSaveLocation: vi.fn(),
  prepareOpenFile: openProtocolMocks.prepareOpenFile,
  saveFile: vi.fn(),
}));

vi.mock("@/composables/unsavedChangesDialog", async () => {
  const actual = await vi.importActual<typeof import("@/composables/unsavedChangesDialog")>(
    "@/composables/unsavedChangesDialog"
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

function openedResponse(fileName: string, documentId: number | string): OpenDocumentResponse {
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
  return openResponseFromFileData(fileData(fileName, "opened"), editorSession);
}

function preparedOpen(
  token = "prepared-open",
  response = openedResponse(`${token}.xlsx`, 2),
): PreparedOpenDocument {
  return preparedOpenDocument(response, token);
}

function openReceipt(response: OpenDocumentResponse) {
  return {
    kind: 'open' as const,
    documentId: response.editorSession.documentId,
    revision: response.editorSession.revision,
    path: response.document.path,
    fileName: response.document.fileName,
  };
}

function closeReceipt(documentId: `${bigint}`, revision: `${bigint}`) {
  return {
    kind: 'close' as const,
    documentId,
    revision,
    path: '/tmp/closed.xlsx',
    fileName: 'closed.xlsx',
  };
}

function mockPreparedOpen(response: OpenDocumentResponse, token = "prepared-open") {
  openProtocolMocks.prepareOpenFile.mockResolvedValue(preparedOpen(token, response));
  openProtocolMocks.commitPreparedDocument.mockResolvedValue(openReceipt(response));
}

function newUntitledResponse(documentId: number): OpenDocumentResponse {
  const response = openedResponse("untitled.xlsx", documentId);
  return { ...response, document: { ...response.document, path: "" } };
}

function savedResponse(fileName: string, path: string, documentId: number): SavedDocumentResponse {
  const opened = openedResponse(fileName, documentId);
  return savedResponseFromFileData(
    { ...fileData(fileName, "saved"), path },
    { ...opened.editorSession, revision: '1' },
  );
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

function controlledCancellation() {
  let cancelled = false;
  const handlers = new Set<() => void>();
  return {
    signal: {
      isCancelled: () => cancelled,
      onCancel(handler: () => void) {
        if (cancelled) {
          handler();
          return () => undefined;
        }
        handlers.add(handler);
        return () => handlers.delete(handler);
      },
    } satisfies OperationCancellationSignal,
    cancel() {
      cancelled = true;
      for (const handler of handlers) handler();
      handlers.clear();
    },
  };
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
  return workspace.run(() => useFileActions({
    fileData: computed(() => documentSessionStore.data),
    flushPendingCellChanges,
  }));
}

describe("useFileActions", () => {
  beforeEach(async () => {
    setActivePinia(createPinia());
    workspace = createApplicationWorkspaceTestContext();
    vi.clearAllMocks();
    const api = await import('@/api');
    vi.mocked(api.closeCurrentDocument).mockImplementation(async (context) =>
      closeReceipt(context.documentId, context.baseRevision));
  });

  afterEach(() => workspace.application.dispose());

  it("does not ask to discard when file picking is cancelled", async () => {
    const platform = await import("@/platform");
    const unsavedChanges = await import("@/composables/unsavedChangesDialog");
    const documentSessionStore = useDocumentSessionStore();
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    openDocumentSession(workspace.runtime, openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    vi.mocked(platform.pickOpenFile).mockResolvedValue(null);

    const actions = mountActions(flushPendingCellChanges);

    await actions.handleOpenFile();

    expect(unsavedChanges.confirmDiscardUnsavedChanges).not.toHaveBeenCalled();
    expect(flushPendingCellChanges).not.toHaveBeenCalled();
    expect(platform.discardOpenFileSelection).not.toHaveBeenCalled();
    expect(platform.prepareOpenFile).not.toHaveBeenCalled();
    expect(documentSessionStore.currentFilePath).toBe("/tmp/current.xlsx");
  });

  it("drops a path load that is cancelled while reading", async () => {
    const api = await import("@/api");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const pendingPrepare = deferred<PreparedOpenDocument>();
    const cancellation = controlledCancellation();
    vi.mocked(platform.prepareOpenFile).mockReturnValue(pendingPrepare.promise);

    const actions = mountActions(vi.fn().mockResolvedValue(true));
    const loadPromise = actions.loadFileFromPath(
      "/tmp/stale.xlsx",
      cancellation.signal,
    );

    await flushPromises();
    expect(platform.prepareOpenFile).toHaveBeenCalledWith(
      "/tmp/stale.xlsx",
      expect.any(String),
    );

    cancellation.cancel();
    await expect(loadPromise).resolves.toBe(false);
    pendingPrepare.resolve(preparedOpen("stale-token"));
    await flushPromises();

    expect(documentSessionStore.documentId).toBeNull();
    expect(documentSessionStore.currentFilePath).toBeNull();
    expect(api.commitPreparedDocument).not.toHaveBeenCalled();
    expect(api.abortPreparedDocument).toHaveBeenCalledWith("stale-token");
    expect(api.addRecentFileWithThumbnail).not.toHaveBeenCalled();
  });

  it("releases the loading lifecycle while a cancelled prepare drains", async () => {
    const api = await import("@/api");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const pendingPrepare = deferred<PreparedOpenDocument>();
    const cancellation = controlledCancellation();
    vi.mocked(platform.prepareOpenFile).mockReturnValue(pendingPrepare.promise);

    const actions = mountActions(vi.fn().mockResolvedValue(true));
    const loadPromise = actions.loadFileFromPath("/tmp/slow.xlsx", cancellation.signal);

    await flushPromises();

    expect(documentSessionStore.lifecycle).toBe("loading");
    expect(documentSessionStore.isInteractionLocked).toBe(true);

    cancellation.cancel();
    await expect(loadPromise).resolves.toBe(false);

    expect(documentSessionStore.lifecycle).toBe("idle");
    expect(documentSessionStore.isInteractionLocked).toBe(false);

    pendingPrepare.resolve(preparedOpen("slow-token"));
    await flushPromises();
    expect(documentSessionStore.lifecycle).toBe("idle");
    expect(documentSessionStore.isInteractionLocked).toBe(false);
    expect(documentSessionStore.documentId).toBeNull();
    expect(documentSessionStore.currentFilePath).toBeNull();
    expect(api.commitPreparedDocument).not.toHaveBeenCalled();
    expect(api.abortPreparedDocument).toHaveBeenCalledWith("slow-token");
    expect(api.addRecentFileWithThumbnail).not.toHaveBeenCalled();
  });

  it("closes an initial document committed after its route load was cancelled", async () => {
    const api = await import("@/api");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const response = openedResponse("committing.xlsx", 2);
    const pendingCommit = deferred<ReturnType<typeof openReceipt>>();
    const cancellation = controlledCancellation();
    vi.mocked(platform.prepareOpenFile).mockResolvedValue(
      preparedOpen("committing-token", response),
    );
    vi.mocked(api.commitPreparedDocument).mockReturnValue(pendingCommit.promise);

    const actions = mountActions(vi.fn().mockResolvedValue(true));
    const loadPromise = actions.loadFileFromPath("/tmp/committing.xlsx", cancellation.signal);
    await waitForCondition(() => vi.mocked(api.commitPreparedDocument).mock.calls.length > 0);

    cancellation.cancel();
    expect(documentSessionStore.lifecycle).toBe("loading");

    pendingCommit.resolve(openReceipt(response));

    await expect(loadPromise).resolves.toBe(false);
    expect(api.closeCurrentDocument).toHaveBeenCalledWith(
      { documentId: '2', baseRevision: '0' },
      expect.any(String),
    );
    expect(documentSessionStore.data).toBeNull();
    expect(documentSessionStore.documentId).toBeNull();
    expect(api.addRecentFileWithThumbnail).not.toHaveBeenCalled();
  });

  it("closes a replacement document committed after its route load was cancelled", async () => {
    const api = await import("@/api");
    const documentSessionStore = useDocumentSessionStore();
    const response = openedResponse("replacement.xlsx", 2);
    const pendingCommit = deferred<ReturnType<typeof openReceipt>>();
    const cancellation = controlledCancellation();
    openDocumentSession(
      workspace.runtime,
      openedResponse("current.xlsx", 1),
      "/tmp/current.xlsx"
    );
    vi.mocked(openProtocolMocks.prepareOpenFile).mockResolvedValue(
      preparedOpen("replacement-token", response)
    );
    vi.mocked(api.commitPreparedDocument).mockReturnValue(pendingCommit.promise);

    const actions = mountActions(vi.fn().mockResolvedValue(true));
    const loadPromise = actions.loadFileFromPath("/tmp/replacement.xlsx", cancellation.signal);
    await waitForCondition(() => vi.mocked(api.commitPreparedDocument).mock.calls.length > 0);

    expect(api.commitPreparedDocument).toHaveBeenCalledWith(
      "replacement-token",
      { documentId: '1', baseRevision: '0' },
      expect.any(String),
    );
    cancellation.cancel();
    pendingCommit.resolve(openReceipt(response));

    await expect(loadPromise).resolves.toBe(false);
    expect(api.closeCurrentDocument).toHaveBeenCalledWith(
      { documentId: '2', baseRevision: '0' },
      expect.any(String),
    );
    expect(documentSessionStore.data).toBeNull();
    expect(documentSessionStore.documentId).toBeNull();
    expect(documentSessionStore.currentFilePath).toBeNull();
    expect(api.addRecentFileWithThumbnail).not.toHaveBeenCalled();
  });

  it("suppresses stale path load errors after cancellation", async () => {
    const elementPlus = await import("element-plus");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const pendingPrepare = deferred<PreparedOpenDocument>();
    const cancellation = controlledCancellation();
    vi.mocked(platform.prepareOpenFile).mockReturnValue(pendingPrepare.promise);

    const actions = mountActions(vi.fn().mockResolvedValue(true));
    const loadPromise = actions.loadFileFromPath("/tmp/stale-error.xlsx", cancellation.signal);

    await flushPromises();

    cancellation.cancel();
    await expect(loadPromise).resolves.toBe(false);
    pendingPrepare.reject(new Error("stale read failed"));
    await flushPromises();

    expect(elementPlus.ElMessage.error).not.toHaveBeenCalled();
    expect(documentSessionStore.data).toBeNull();
    expect(documentSessionStore.lifecycle).toBe("idle");
  });

  it("waits for an active document lifecycle before loading a route file path", async () => {
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    workspace.runtime.session.beginLifecycle('saving');
    mockPreparedOpen(openedResponse("queued.xlsx", 2));

    const actions = mountActions(flushPendingCellChanges);
    const loadPromise = actions.loadFileFromPath("/tmp/queued.xlsx");

    await flushPromises();

    expect(platform.prepareOpenFile).not.toHaveBeenCalled();
    expect(documentSessionStore.lifecycle).toBe("saving");

    workspace.runtime.session.endLifecycle('saving');

    await expect(loadPromise).resolves.toBe(true);
    expect(platform.prepareOpenFile).toHaveBeenCalledWith(
      "/tmp/queued.xlsx",
      expect.any(String),
    );
    expect(documentSessionStore.currentFilePath).toBe("/tmp/queued.xlsx");
  });

  it("waits for an active editor command before loading a route file path", async () => {
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    const releaseEditorCommand = workspace.runtime.session.beginEditorCommand();
    mockPreparedOpen(openedResponse("queued.xlsx", 2));

    const actions = mountActions(flushPendingCellChanges);
    const loadPromise = actions.loadFileFromPath("/tmp/queued.xlsx");

    await flushPromises();

    expect(platform.prepareOpenFile).not.toHaveBeenCalled();
    expect(documentSessionStore.lifecycle).toBe("idle");
    expect(documentSessionStore.isInteractionLocked).toBe(true);

    releaseEditorCommand?.();

    await expect(loadPromise).resolves.toBe(true);
    expect(platform.prepareOpenFile).toHaveBeenCalledWith(
      "/tmp/queued.xlsx",
      expect.any(String),
    );
    expect(documentSessionStore.currentFilePath).toBe("/tmp/queued.xlsx");
  });

  it("does not block path loading on recent file metadata updates", async () => {
    const api = await import("@/api");
    const documentSessionStore = useDocumentSessionStore();
    const recentUpdate = deferred<Awaited<ReturnType<typeof api.addRecentFileWithThumbnail>>>();
    vi.mocked(api.addRecentFileWithThumbnail).mockReturnValue(recentUpdate.promise);
    mockPreparedOpen(openedResponse("fast.xlsx", 2));

    const actions = mountActions(vi.fn().mockResolvedValue(true));

    await expect(actions.loadFileFromPath("/tmp/fast.xlsx")).resolves.toBe(true);

    expect(documentSessionStore.currentFilePath).toBe("/tmp/fast.xlsx");
    expect(api.addRecentFileWithThumbnail).toHaveBeenCalledWith(
      { documentId: '2', baseRevision: '0' },
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
    const documentSessionStore = useDocumentSessionStore();
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    mockPreparedOpen(openedResponse("nameless.xlsx", 2));
    vi.mocked(api.addRecentFileWithThumbnail).mockRejectedValueOnce(
      new Error("metadata unavailable")
    );

    try {
      const actions = mountActions(vi.fn().mockResolvedValue(true));

      await expect(actions.loadFileFromPath("/tmp/nameless.xlsx")).resolves.toBe(true);
      await flushPromises();

      expect(documentSessionStore.documentId).toBe('2');
      expect(documentSessionStore.currentFilePath).toBe("/tmp/nameless.xlsx");
      expect(api.addRecentFileWithThumbnail).toHaveBeenCalledWith(
        { documentId: '2', baseRevision: '0' },
        undefined
      );
      expect(warn).toHaveBeenCalled();
    } finally {
      warn.mockRestore();
    }
  });

  it("asks before opening when pending work becomes dirty after flush", async () => {
    const platform = await import("@/platform");
    const unsavedChanges = await import("@/composables/unsavedChangesDialog");
    const documentSessionStore = useDocumentSessionStore();
    const statusStore = useDocumentStatusStore();
    openDocumentSession(workspace.runtime, openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    vi.mocked(unsavedChanges.confirmDiscardUnsavedChanges).mockResolvedValue(false);
    vi.mocked(platform.pickOpenFile).mockResolvedValue({
      path: "/tmp/next.xlsx",
      fileName: "next.xlsx",
    });

    const actions = mountActions(async () => {
      statusStore.applyEditorState({
        canUndo: true,
        canRedo: false,
        isContentDirty: true,
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
    expect(platform.prepareOpenFile).not.toHaveBeenCalled();
    expect(documentSessionStore.currentFilePath).toBe("/tmp/current.xlsx");
  });

  it("does not flush discarded work when the current document is already dirty", async () => {
    const platform = await import("@/platform");
    const unsavedChanges = await import("@/composables/unsavedChangesDialog");
    const documentSessionStore = useDocumentSessionStore();
    const statusStore = useDocumentStatusStore();
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    openDocumentSession(workspace.runtime, openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    statusStore.applyEditorState({
      canUndo: true,
      canRedo: false,
      isContentDirty: true,
      history: defaultHistoryStatus(),
    });
    vi.mocked(unsavedChanges.confirmDiscardUnsavedChanges).mockResolvedValue(true);
    vi.mocked(platform.pickOpenFile).mockResolvedValue({
      path: "/tmp/next.xlsx",
      fileName: "next.xlsx",
    });
    mockPreparedOpen(openedResponse("next.xlsx", 2));

    const actions = mountActions(flushPendingCellChanges);

    await actions.handleOpenFile();

    expect(unsavedChanges.confirmDiscardUnsavedChanges).toHaveBeenCalledTimes(1);
    expect(flushPendingCellChanges).not.toHaveBeenCalled();
    expect(platform.pickOpenFile).toHaveBeenCalledTimes(1);
    expect(platform.discardOpenFileSelection).not.toHaveBeenCalled();
    expect(platform.prepareOpenFile).toHaveBeenCalledWith(
      "/tmp/next.xlsx",
      expect.any(String),
    );
    expect(documentSessionStore.documentId).toBe('2');
  });

  it("discards newly dirty pending work before opening the selected file", async () => {
    const platform = await import("@/platform");
    const unsavedChanges = await import("@/composables/unsavedChangesDialog");
    const documentSessionStore = useDocumentSessionStore();
    const statusStore = useDocumentStatusStore();
    openDocumentSession(workspace.runtime, openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    vi.mocked(unsavedChanges.confirmDiscardUnsavedChanges).mockResolvedValue(true);
    vi.mocked(platform.pickOpenFile).mockResolvedValue({
      path: "/tmp/next.xlsx",
      fileName: "next.xlsx",
    });
    mockPreparedOpen(openedResponse("next.xlsx", 2));

    const actions = mountActions(async () => {
      statusStore.applyEditorState({
        canUndo: true,
        canRedo: false,
        isContentDirty: true,
        history: defaultHistoryStatus(),
      });
      return true;
    });

    await actions.handleOpenFile();

    expect(unsavedChanges.confirmDiscardUnsavedChanges).toHaveBeenCalledTimes(1);
    expect(platform.pickOpenFile).toHaveBeenCalledTimes(1);
    expect(platform.discardOpenFileSelection).not.toHaveBeenCalled();
    expect(platform.prepareOpenFile).toHaveBeenCalledWith(
      "/tmp/next.xlsx",
      expect.any(String),
    );
    expect(documentSessionStore.documentId).toBe('2');
    expect(documentSessionStore.currentFilePath).toBe("/tmp/next.xlsx");
  });

  it("discards selected imported files when reading fails", async () => {
    const platform = await import("@/platform");
    const unsavedChanges = await import("@/composables/unsavedChangesDialog");
    const documentSessionStore = useDocumentSessionStore();
    const statusStore = useDocumentStatusStore();
    const pendingCellSavesStore = usePendingCellSavesStore();
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    const selection = {
      path: "/tmp/broken.xlsx",
      fileName: "broken.xlsx",
      originalPath: "content://broken",
    };
    openDocumentSession(workspace.runtime, openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    statusStore.markPendingContentChange();
    pendingCellSavesStore.applyDraft(
      "0,0,0",
      { sheetIndex: 0, row: 0, col: 0, value: "draft", oldValue: text("current") },
      text("current")
    );
    vi.mocked(unsavedChanges.confirmDiscardUnsavedChanges).mockResolvedValue(true);
    vi.mocked(platform.pickOpenFile).mockResolvedValue(selection);
    vi.mocked(platform.prepareOpenFile).mockRejectedValue(new Error("cannot parse"));

    const actions = mountActions(flushPendingCellChanges);

    await actions.handleOpenFile();

    expect(platform.prepareOpenFile).toHaveBeenCalledWith(
      "/tmp/broken.xlsx",
      expect.any(String),
    );
    expect(unsavedChanges.confirmDiscardUnsavedChanges).toHaveBeenCalledTimes(1);
    expect(flushPendingCellChanges).not.toHaveBeenCalled();
    expect(platform.discardOpenFileSelection).toHaveBeenCalledWith(selection);
    expect(documentSessionStore.currentFilePath).toBe("/tmp/current.xlsx");
    expect(statusStore.hasPendingContentChange).toBe(true);
    expect(pendingCellSavesStore.draftCellValues["0,0,0"]).toBe("draft");
  });

  it("allows closing a stale projection after discard confirmation", async () => {
    const api = await import("@/api");
    const unsavedChanges = await import("@/composables/unsavedChangesDialog");
    const documentSessionStore = useDocumentSessionStore();
    const statusStore = useDocumentStatusStore();
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    openDocumentSession(workspace.runtime, openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    statusStore.applyEditorState({
      canUndo: true,
      canRedo: false,
      isContentDirty: true,
      history: defaultHistoryStatus(),
    });
    documentSessionStore.projectionStale = true;
    vi.mocked(unsavedChanges.confirmDiscardUnsavedChanges).mockResolvedValue(true);

    const actions = mountActions(flushPendingCellChanges);

    await expect(actions.closeCurrentDocument()).resolves.toBe(true);

    expect(api.closeCurrentDocument).toHaveBeenCalledWith(
      { documentId: '1', baseRevision: '0' },
      expect.any(String),
    );
    expect(flushPendingCellChanges).not.toHaveBeenCalled();
    expect(documentSessionStore.data).toBeNull();
    expect(documentSessionStore.documentId).toBeNull();
    expect(documentSessionStore.projectionStale).toBe(false);
  });

  it("locks document interaction while backend close is pending", async () => {
    const api = await import("@/api");
    const documentSessionStore = useDocumentSessionStore();
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    const pendingClose = deferred<ReturnType<typeof closeReceipt>>();
    openDocumentSession(workspace.runtime, openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    vi.mocked(api.closeCurrentDocument).mockReturnValue(pendingClose.promise);

    const actions = mountActions(flushPendingCellChanges);
    const closePromise = actions.closeCurrentDocument();

    await flushPromises();

    expect(documentSessionStore.lifecycle).toBe("closing");
    expect(documentSessionStore.isInteractionLocked).toBe(true);

    await expect(actions.closeCurrentDocument()).resolves.toBe(false);
    expect(api.closeCurrentDocument).toHaveBeenCalledTimes(1);

    pendingClose.resolve(closeReceipt('1', '0'));

    await expect(closePromise).resolves.toBe(true);
    expect(documentSessionStore.lifecycle).toBe("idle");
    expect(documentSessionStore.data).toBeNull();
  });

  it("waits for an active lifecycle before closing for application exit", async () => {
    const api = await import("@/api");
    const documentSessionStore = useDocumentSessionStore();
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    openDocumentSession(workspace.runtime, openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    expect(workspace.runtime.session.beginLifecycle('saving')).toBe(true);

    const actions = mountActions(flushPendingCellChanges);
    const closePromise = actions.closeCurrentDocument({ waitForIdle: true });

    await flushPromises();
    expect(api.closeCurrentDocument).not.toHaveBeenCalled();

    workspace.runtime.session.endLifecycle('saving');

    await expect(closePromise).resolves.toBe(true);
    expect(api.closeCurrentDocument).toHaveBeenCalledWith(
      { documentId: '1', baseRevision: '0' },
      expect.any(String),
    );
    expect(documentSessionStore.data).toBeNull();
  });

  it("keeps the document locked until application exit preparation is settled", async () => {
    const documentSessionStore = useDocumentSessionStore();
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    openDocumentSession(workspace.runtime, openedResponse("current.xlsx", 1), "/tmp/current.xlsx");

    const actions = mountActions(flushPendingCellChanges);
    const preparation = await actions.prepareApplicationExit({ waitForIdle: true });

    expect(preparation).not.toBeNull();
    expect(documentSessionStore.lifecycle).toBe("closing");
    expect(documentSessionStore.isInteractionLocked).toBe(true);
    expect(documentSessionStore.data).not.toBeNull();

    preparation?.rollback();

    expect(documentSessionStore.lifecycle).toBe("idle");
    expect(documentSessionStore.isInteractionLocked).toBe(false);
    expect(documentSessionStore.data).not.toBeNull();
  });

  it("delegates document closing to the route-leave guard when returning home", async () => {
    const api = await import("@/api");
    const documentSessionStore = useDocumentSessionStore();
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    const pendingNavigation = deferred<void>();
    let backResolved = false;
    openDocumentSession(workspace.runtime, openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    routerMocks.push.mockReturnValue(pendingNavigation.promise);

    const actions = mountActions(flushPendingCellChanges);
    const backPromise = actions.handleBack().then(() => {
      backResolved = true;
    });

    await waitForCondition(() => routerMocks.push.mock.calls.length > 0);

    expect(api.closeCurrentDocument).not.toHaveBeenCalled();
    expect(routerMocks.push).toHaveBeenCalledWith({ name: "home" });
    expect(documentSessionStore.documentId).toBe('1');
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
    openDocumentSession(workspace.runtime, openedResponse("current.xlsx", 1), "/tmp/current.xlsx");
    routerMocks.push.mockRejectedValue(new Error("navigation failed"));

    const actions = mountActions(flushPendingCellChanges);

    await expect(actions.handleBack()).resolves.toBeUndefined();

    expect(api.closeCurrentDocument).not.toHaveBeenCalled();
    expect(routerMocks.push).toHaveBeenCalledWith({ name: "home" });
    expect(documentSessionStore.documentId).toBe('1');
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
    openDocumentSession(workspace.runtime, newUntitledResponse(1), null);
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
    openDocumentSession(workspace.runtime, newUntitledResponse(1), null);
    vi.mocked(api.getNativeSavePlan)
      .mockResolvedValueOnce(nativeSavePlan({ requiresSaveAs: true }))
      .mockResolvedValueOnce(nativeSavePlan());
    vi.mocked(platform.pickSaveLocation).mockResolvedValue(savePath);
    vi.mocked(platform.saveFile).mockResolvedValue(savedResponse("saved.xlsx", savePath, 1));

    const actions = mountActions(flushPendingCellChanges);

    await actions.handleSaveFile();

    expect(platform.saveFile).toHaveBeenCalledWith(savePath, {
      documentId: '1',
      baseRevision: '0',
    }, expect.any(String));
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
    openDocumentSession(workspace.runtime, newUntitledResponse(1), null);
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
        documentId: '1',
        baseRevision: '0',
      }, expect.any(String));
      expect(platform.discardSaveLocation).not.toHaveBeenCalled();
      expect(documentSessionStore.currentFilePath).toBe(savePath);
      expect(api.addRecentFileWithThumbnail).toHaveBeenCalledWith(
        { documentId: '1', baseRevision: '1' },
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
    openDocumentSession(workspace.runtime, openedResponse("current.xlsx", 1), existingPath);
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
        documentId: '1',
        baseRevision: '0',
      }, expect.any(String));
      expect(documentSessionStore.currentFilePath).toBe(existingPath);
      expect(api.addRecentFileWithThumbnail).toHaveBeenCalledWith(
        { documentId: '1', baseRevision: '1' },
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
    openDocumentSession(workspace.runtime, openedResponse("current.xlsx", 1), existingPath);
    documentSessionStore.revision = '3';
    documentSessionStore.projectionStale = true;
    vi.mocked(api.getNativeSavePlan).mockResolvedValueOnce(nativeSavePlan());
    const saved = savedResponse("current.xlsx", existingPath, 1);
    vi.mocked(platform.saveFile).mockResolvedValue({
      ...saved,
      editorSession: {
        ...saved.editorSession,
        revision: '4',
      },
    });

    const actions = mountActions(flushPendingCellChanges);

    await actions.handleSaveFile();

    expect(platform.saveFile).toHaveBeenCalledWith(existingPath, {
      documentId: '1',
      baseRevision: '3',
    }, expect.any(String));
    expect(documentSessionStore.projectionStale).toBe(false);
    expect(documentSessionStore.loadedSheet(0)?.blocks).toHaveLength(0);
    expect(documentSessionStore.data?.fileName).toBe("current.xlsx");
    expect(elementPlus.ElMessage.success).toHaveBeenCalledWith("File saved successfully");
  });

  it("keeps a reserved save-as location when a successful save response is ignored as stale", async () => {
    const api = await import("@/api");
    const elementPlus = await import("element-plus");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const savePath = "/tmp/stale-response.xlsx";
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    openDocumentSession(workspace.runtime, newUntitledResponse(1), null);
    vi.mocked(api.getNativeSavePlan)
      .mockResolvedValueOnce(nativeSavePlan({ requiresSaveAs: true }))
      .mockResolvedValueOnce(nativeSavePlan());
    vi.mocked(platform.pickSaveLocation).mockResolvedValue(savePath);
    vi.mocked(platform.saveFile).mockImplementation(async () => {
      documentSessionStore.documentId = '2';
      return savedResponse("stale-response.xlsx", savePath, 1);
    });

    const actions = mountActions(flushPendingCellChanges);

    await actions.handleSaveFile();

    expect(platform.saveFile).toHaveBeenCalledWith(savePath, {
      documentId: '1',
      baseRevision: '0',
    }, expect.any(String));
    expect(platform.discardSaveLocation).not.toHaveBeenCalled();
    expect(elementPlus.ElMessage.warning).toHaveBeenCalledWith(
      "File was saved, but the active document changed before the editor could refresh."
    );
    expect(elementPlus.ElMessage.success).not.toHaveBeenCalled();
    expect(api.addRecentFileWithThumbnail).not.toHaveBeenCalled();
    expect(documentSessionStore.documentId).toBe('2');
    expect(documentSessionStore.currentFilePath).toBeNull();
  });

  it("warns when an existing-file save succeeds but its response is ignored as stale", async () => {
    const api = await import("@/api");
    const elementPlus = await import("element-plus");
    const platform = await import("@/platform");
    const documentSessionStore = useDocumentSessionStore();
    const existingPath = "/tmp/current.xlsx";
    const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
    openDocumentSession(workspace.runtime, openedResponse("current.xlsx", 1), existingPath);
    vi.mocked(api.getNativeSavePlan).mockResolvedValueOnce(nativeSavePlan());
    vi.mocked(platform.saveFile).mockImplementation(async () => {
      documentSessionStore.documentId = '2';
      return savedResponse("current.xlsx", existingPath, 1);
    });

    const actions = mountActions(flushPendingCellChanges);

    await actions.handleSaveFile();

    expect(platform.saveFile).toHaveBeenCalledWith(existingPath, {
      documentId: '1',
      baseRevision: '0',
    }, expect.any(String));
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
    openDocumentSession(workspace.runtime, newUntitledResponse(1), null);
    vi.mocked(api.getNativeSavePlan)
      .mockResolvedValueOnce(nativeSavePlan({ requiresSaveAs: true }))
      .mockResolvedValueOnce(nativeSavePlan());
    vi.mocked(platform.pickSaveLocation).mockResolvedValue(savePath);
    vi.mocked(platform.saveFile).mockRejectedValue(new Error("disk full"));

    const actions = mountActions(flushPendingCellChanges);

    await actions.handleSaveFile();

    expect(platform.saveFile).toHaveBeenCalledWith(savePath, {
      documentId: '1',
      baseRevision: '0',
    }, expect.any(String));
    expect(platform.discardSaveLocation).toHaveBeenCalledWith(savePath);
    expect(documentSessionStore.currentFilePath).toBeNull();
  });
});
