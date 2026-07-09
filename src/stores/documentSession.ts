import type {
  EditorMutationResponse,
  EditorSessionInfo,
  FileData,
  OpenDocumentResponse,
  SavedDocumentResponse,
  EditorCommandContext,
  EditorPatch,
} from "@/types";
import { applyDocumentPatches } from "@/stores/documentPatches";
import { usePendingCellSavesStore } from "@/stores/pendingCellSaves";
import { useSearchSessionStore } from "@/stores/searchSession";
import { useDocumentStatusStore } from "@/stores/documentStatus";
import type { DocumentStatusSnapshot } from "@/stores/documentStatus";
import { useEditorSelectionStore } from "@/stores/editorSelection";
import type { EditorSelectionSnapshot } from "@/stores/editorSelection";
import { calculateSheetExtent } from "@/table-geometry/sheetExtent";

export type MutationApplyResult = {
  data: FileData | null;
  resyncRequired: boolean;
};

export type DocumentSessionLifecycle = "idle" | "loading" | "saving";

type DocumentSessionRuntime = {
  tail: Promise<void> | null;
  lifecycleIdleWaiters: Array<() => void>;
};

type DocumentSessionSnapshot = {
  data: FileData | null;
  currentFilePath: string | null;
  documentId: number | null;
  revision: number;
  lifecycle: DocumentSessionLifecycle;
  projectionStale: boolean;
  status: DocumentStatusSnapshot;
  selection: EditorSelectionSnapshot;
};

const documentSessionRuntimes = new WeakMap<object, DocumentSessionRuntime>();

export const useDocumentSessionStore = defineStore("documentSession", {
  state: () => ({
    data: null as FileData | null,
    currentFilePath: null as string | null,
    documentId: null as number | null,
    revision: 0,
    lifecycle: "idle" as DocumentSessionLifecycle,
    projectionStale: false,
  }),
  getters: {
    isInteractionLocked: (state) => state.lifecycle !== "idle",
    isEditorInteractionLocked: (state) => state.lifecycle !== "idle" || state.projectionStale,
  },
  actions: {
    beginLifecycle(lifecycle: Exclude<DocumentSessionLifecycle, "idle">): boolean {
      if (this.lifecycle !== "idle") {
        return false;
      }
      this.lifecycle = lifecycle;
      return true;
    },
    endLifecycle(lifecycle: Exclude<DocumentSessionLifecycle, "idle">) {
      if (this.lifecycle === lifecycle) {
        this.lifecycle = "idle";
        resolveLifecycleIdleWaiters(this);
      }
    },
    waitForIdleLifecycle(): Promise<void> {
      if (this.lifecycle === "idle") {
        return Promise.resolve();
      }
      return new Promise((resolve) => {
        sessionRuntimeFor(this).lifecycleIdleWaiters.push(resolve);
      });
    },
    enqueueDocumentMutation<T>(
      documentId: number,
      task: (context: EditorCommandContext) => Promise<T>
    ): Promise<T | undefined> {
      return enqueueMutation(this, async () => {
        if (this.projectionStale) {
          throw new Error("Document projection is stale; refresh the document before editing.");
        }
        const context = this.commandContextForDocument(documentId);
        if (!context) {
          return undefined;
        }
        return task(context);
      });
    },
    waitForMutations(): Promise<void> {
      return sessionRuntimeFor(this).tail ?? Promise.resolve();
    },
    currentCommandContext(): EditorCommandContext | null {
      if (this.documentId === null) return null;
      return {
        documentId: this.documentId,
        baseRevision: this.revision,
      };
    },
    commandContextForDocument(documentId: number): EditorCommandContext | null {
      const context = this.currentCommandContext();
      if (!context || context.documentId !== documentId) {
        return null;
      }
      return context;
    },
    requireCommandContext(): EditorCommandContext {
      const context = this.currentCommandContext();
      if (!context) {
        throw new Error("No active editor document");
      }
      return context;
    },
    matchesCommandContext(context: EditorCommandContext): boolean {
      return this.documentId === context.documentId && this.revision === context.baseRevision;
    },
    discardPendingLocalWork() {
      resetTransientDocumentWork(this);
    },
    openDocumentResponse(response: OpenDocumentResponse, path: string | null = null) {
      resetTransientDocumentWork(this);
      this.data = response.fileData;
      this.currentFilePath = path !== null ? path : response.fileData.path || null;
      this.documentId = response.editorSession.documentId;
      this.revision = response.editorSession.revision;
      this.projectionStale = false;
      resetSessionUi();
      const statusStore = useDocumentStatusStore();
      statusStore.reset();
      statusStore.applyEditorSession(response.editorSession);
    },
    applySavedDocumentResponse(response: SavedDocumentResponse, path: string | null = null) {
      resetTransientDocumentWork(this);
      this.data = response.fileData;
      this.currentFilePath = path !== null ? path : response.fileData.path || null;
      this.documentId = response.editorSession.documentId;
      this.revision = response.editorSession.revision;
      this.projectionStale = false;
      clampSelectionToCurrentSheet(this);
      useSearchSessionStore().reset();
      useDocumentStatusStore().applyEditorSession(response.editorSession);
    },
    applySavedDocumentResponseForContext(
      context: EditorCommandContext,
      response: SavedDocumentResponse,
      path: string | null = null
    ): boolean {
      if (
        response.editorSession.documentId !== context.documentId
        || response.editorSession.revision < context.baseRevision
        || !this.matchesCommandContext(context)
      ) {
        return false;
      }
      this.applySavedDocumentResponse(response, path);
      return true;
    },
    updateIdentity(path: string | null, fileName: string) {
      if (this.data) {
        this.data = {
          ...this.data,
          path: path ?? this.data.path,
          fileName,
        };
      }
      this.currentFilePath = path;
    },
    clearDocument() {
      resetTransientDocumentWork(this);
      this.data = null;
      this.currentFilePath = null;
      this.documentId = null;
      this.revision = 0;
      this.lifecycle = "idle";
      this.projectionStale = false;
      resolveLifecycleIdleWaiters(this);
      resetSessionUi();
      useDocumentStatusStore().reset();
    },
    applyMutationResponse(response: EditorMutationResponse): MutationApplyResult {
      if (response.protocolVersion !== 1) {
        throw new Error(`Unsupported editor mutation protocol: ${response.protocolVersion}`);
      }
      if (this.documentId !== null && response.documentId !== this.documentId) {
        return { data: this.data, resyncRequired: false };
      }
      if (this.documentId === null && this.data === null) {
        return { data: this.data, resyncRequired: false };
      }
      if (this.documentId === null) {
        this.documentId = response.documentId;
      }
      if (response.revision < this.revision) {
        return { data: this.data, resyncRequired: false };
      }
      if (response.revision > this.revision + 1) {
        this.revision = response.revision;
        applyResponseStatus(response);
        this.projectionStale = true;
        useSearchSessionStore().clearSearch();
        return { data: this.data, resyncRequired: true };
      }
      if (response.revision === this.revision && response.patches?.length) {
        applyResponseStatus(response);
        this.projectionStale = true;
        useSearchSessionStore().clearSearch();
        return { data: this.data, resyncRequired: true };
      }
      if (response.revision === this.revision) {
        applyResponseStatus(response);
        return { data: this.data, resyncRequired: false };
      }
      applyResponseStatus(response);
      this.revision = response.revision;
      try {
        const result = applyDocumentPatches(this.data, response.patches);
        this.data = result.data;
        useEditorSelectionStore().applyEditorPatches(response.patches);
        if (mutationInvalidatesSearch(response.patches)) {
          useSearchSessionStore().clearSearch();
        }
        clampSelectionToCurrentSheet(this);
        return {
          data: result.data,
          resyncRequired: result.resyncRequired,
        };
      } catch (error) {
        this.projectionStale = true;
        useSearchSessionStore().clearSearch();
        throw error;
      }
    },
    markProjectionStaleFromMutationResponse(response: EditorMutationResponse): boolean {
      if (this.documentId !== null && response.documentId !== this.documentId) {
        return false;
      }
      if (this.documentId === null && this.data === null) {
        return false;
      }
      if (response.revision < this.revision) {
        return false;
      }
      if (this.documentId === null) {
        this.documentId = response.documentId;
      }
      this.revision = response.revision;
      if (response.protocolVersion === 1) {
        applyResponseStatus(response);
      }
      this.projectionStale = true;
      useSearchSessionStore().clearSearch();
      return true;
    },
    async applyMutationResponseWithResync(
      response: EditorMutationResponse,
      fetchProjection: (context: EditorCommandContext) => Promise<FileData>
    ): Promise<MutationApplyResult> {
      const snapshot = captureMutationSnapshot(this);
      const result = this.applyMutationResponse(response);
      if (!result.resyncRequired) {
        return result;
      }
      const resyncContext = {
        documentId: response.documentId,
        baseRevision: response.revision,
      };
      try {
        const projection = await fetchProjection(resyncContext);
        if (!this.matchesCommandContext(resyncContext)) {
          return {
            data: this.data,
            resyncRequired: true,
          };
        }
        replaceProjection(this, projection);
      } catch (error) {
        if (this.matchesCommandContext(resyncContext)) {
          restoreMutationSnapshot(this, snapshot);
          this.documentId = response.documentId;
          this.revision = response.revision;
          applyResponseStatus(response);
          this.projectionStale = true;
        }
        throw error;
      }
      return {
        data: this.data,
        resyncRequired: true,
      };
    },
    async refreshAfterMutationFailure(
      fetchEditorSession: (
        context: EditorCommandContext | null
      ) => Promise<EditorSessionInfo | null | undefined>,
      fetchProjection?: (context: EditorCommandContext) => Promise<FileData>
    ) {
      const context = this.currentCommandContext();
      if (!fetchProjection || !context) {
        this.applyEditorSessionForContext(context, await fetchEditorSession(context));
        return;
      }

      const snapshot = captureMutationSnapshot(this);
      try {
        const [projection, session] = await Promise.all([
          fetchProjection(context),
          fetchEditorSession(context),
        ]);
        if (!this.matchesCommandContext(context)) {
          return;
        }
        replaceProjection(this, projection);
        this.applyEditorSessionForContext(context, session);
      } catch (error) {
        if (this.matchesCommandContext(context)) {
          restoreMutationSnapshot(this, snapshot);
        }
        throw error;
      }
    },
    applyEditorSessionForContext(
      context: EditorCommandContext | null,
      info: EditorSessionInfo | null | undefined
    ) {
      if (context) {
        if (!this.matchesCommandContext(context)) {
          return;
        }
        this.applyEditorSession(info);
        return;
      }

      if (this.documentId !== null) {
        return;
      }
      if (!info) {
        this.clearDocument();
        return;
      }
      if (this.data !== null) {
        this.applyEditorSession(info);
      }
    },
    applyEditorSession(info: EditorSessionInfo | null | undefined) {
      if (!info) {
        this.clearDocument();
        return;
      }
      if (this.data === null) {
        return;
      }
      if (this.documentId !== null && info.documentId !== this.documentId) {
        return;
      }
      const revisionAdvancedWithoutProjection = info.revision > this.revision;
      this.documentId = info.documentId;
      this.revision = Math.max(this.revision, info.revision);
      useDocumentStatusStore().applyEditorSession(info);
      if (revisionAdvancedWithoutProjection) {
        this.projectionStale = true;
        useSearchSessionStore().clearSearch();
      }
    },
  },
});

type DocumentSessionStateTarget = {
  data: FileData | null;
  currentFilePath: string | null;
  documentId: number | null;
  revision: number;
  lifecycle: DocumentSessionLifecycle;
  projectionStale: boolean;
};

function resetMutationQueue(store: object) {
  sessionRuntimeFor(store).tail = null;
}

function enqueueMutation<T>(store: object, task: () => Promise<T>): Promise<T> {
  const runtime = sessionRuntimeFor(store);
  const tail = runtime.tail ?? Promise.resolve();
  const run = tail.then(task, task);
  const cleanup = run.then(
    () => undefined,
    () => undefined
  );
  runtime.tail = cleanup;
  cleanup.finally(() => {
    if (runtime.tail === cleanup) {
      runtime.tail = null;
    }
  });
  return run;
}

function resetTransientDocumentWork(store: object) {
  resetMutationQueue(store);
  usePendingCellSavesStore().reset();
  useDocumentStatusStore().clearPendingContentChange();
}

function replaceProjection(store: DocumentSessionStateTarget, data: FileData) {
  const currentFileName = store.data?.fileName;
  store.data = {
    ...data,
    path: store.currentFilePath ?? data.path,
    fileName: currentFileName ?? data.fileName,
  };
  store.projectionStale = false;
  clampSelectionToCurrentSheet(store);
  useSearchSessionStore().clearSearch();
}

function applyResponseStatus(response: EditorMutationResponse) {
  const statusStore = useDocumentStatusStore();
  statusStore.applyRuntimeStatus(response.formulaStatus, response.capabilities);
  statusStore.applyEditorState(response.editorState);
}

function captureMutationSnapshot(store: DocumentSessionStateTarget): DocumentSessionSnapshot {
  const statusStore = useDocumentStatusStore();
  const selectionStore = useEditorSelectionStore();
  return {
    data: store.data,
    currentFilePath: store.currentFilePath,
    documentId: store.documentId,
    revision: store.revision,
    lifecycle: store.lifecycle,
    projectionStale: store.projectionStale,
    status: statusStore.captureSnapshot(),
    selection: selectionStore.captureSnapshot(),
  };
}

function restoreMutationSnapshot(
  store: DocumentSessionStateTarget,
  snapshot: DocumentSessionSnapshot
) {
  store.data = snapshot.data;
  store.currentFilePath = snapshot.currentFilePath;
  store.documentId = snapshot.documentId;
  store.revision = snapshot.revision;
  store.lifecycle = snapshot.lifecycle;
  store.projectionStale = snapshot.projectionStale;

  useDocumentStatusStore().restoreSnapshot(snapshot.status);
  useEditorSelectionStore().restoreSnapshot(snapshot.selection);
}

function resetSessionUi() {
  useEditorSelectionStore().reset();
  useSearchSessionStore().reset();
  usePendingCellSavesStore().reset();
}

function clampSelectionToCurrentSheet(store: DocumentSessionStateTarget) {
  const selectionStore = useEditorSelectionStore();
  if (!store.data) {
    selectionStore.clearSelection();
    return;
  }
  selectionStore.clampToSheetData(store.data.sheets.length, (sheetIndex, row, col) => {
    const sheet = store.data?.sheets[sheetIndex];
    if (!sheet) return false;
    const extent = calculateSheetExtent(
      sheet.rows,
      sheet.merges,
      sheet.columnWidths,
      sheet.rowHeights,
      sheet.rich
    );
    return row >= 0 && col >= 0 && row < extent.rowCount && col < extent.columnCount;
  });
}

function sessionRuntimeFor(store: object): DocumentSessionRuntime {
  let runtime = documentSessionRuntimes.get(store);
  if (!runtime) {
    runtime = { tail: null, lifecycleIdleWaiters: [] };
    documentSessionRuntimes.set(store, runtime);
  }
  return runtime;
}

function resolveLifecycleIdleWaiters(store: object) {
  const runtime = sessionRuntimeFor(store);
  const waiters = runtime.lifecycleIdleWaiters.splice(0);
  for (const resolve of waiters) {
    resolve();
  }
}

function mutationInvalidatesSearch(patches: EditorPatch[] | undefined): boolean {
  return (patches ?? []).some((patch) => patch.type !== "Layout");
}
