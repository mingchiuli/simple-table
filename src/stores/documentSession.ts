import type {
  EditorMutationResponse,
  EditorSessionInfo,
  FileData,
  FormulaStatus,
  HistoryStatus,
  OpenDocumentResponse,
  SavedDocumentResponse,
  EditorCommandContext,
  WorkbookCapabilities,
  EditorPatch,
} from "@/types";
import { applyDocumentPatches } from "@/stores/documentPatches";
import { usePendingCellSavesStore } from "@/stores/pendingCellSaves";
import { useSearchSessionStore } from "@/stores/searchSession";
import { useDocumentStatusStore } from "@/stores/documentStatus";
import { useEditorSelectionStore } from "@/stores/editorSelection";
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

type CellPosition = { row: number; col: number };

type DocumentStatusSnapshot = {
  canUndo: boolean;
  canRedo: boolean;
  isContentDirty: boolean;
  hasPendingContentChange: boolean;
  formulaStatus: FormulaStatus;
  capabilities: WorkbookCapabilities;
  history: HistoryStatus;
};

type EditorSelectionSnapshot = {
  currentSheetIndex: number;
  selectedCell: CellPosition | null;
  cellEditorValue: string;
  autoScroll: boolean;
  sheetSelectedCells: Map<number, CellPosition>;
};

type DocumentSessionSnapshot = {
  data: FileData | null;
  currentFilePath: string | null;
  documentId: number | null;
  revision: number;
  lifecycle: DocumentSessionLifecycle;
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
  }),
  getters: {
    isInteractionLocked: (state) => state.lifecycle !== "idle",
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
    resetMutationQueue() {
      sessionRuntimeFor(this).tail = null;
    },
    enqueueMutation<T>(task: () => Promise<T>): Promise<T> {
      const runtime = sessionRuntimeFor(this);
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
    },
    enqueueDocumentMutation<T>(
      documentId: number,
      task: (context: EditorCommandContext) => Promise<T>
    ): Promise<T | undefined> {
      return this.enqueueMutation(async () => {
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
      this.resetTransientDocumentWork();
    },
    openDocument(data: FileData, path: string | null = null) {
      this.resetTransientDocumentWork();
      this.data = data;
      this.currentFilePath = path;
      this.documentId = null;
      this.revision = 0;
      this.resetSessionUi();
      useDocumentStatusStore().reset();
    },
    openDocumentResponse(response: OpenDocumentResponse, path: string | null = null) {
      this.resetTransientDocumentWork();
      this.data = response.fileData;
      this.currentFilePath = path !== null ? path : response.fileData.path || null;
      this.documentId = response.editorSession.documentId;
      this.revision = response.editorSession.revision;
      this.resetSessionUi();
      const statusStore = useDocumentStatusStore();
      statusStore.reset();
      statusStore.applyEditorSession(response.editorSession);
    },
    applySavedDocumentResponse(response: SavedDocumentResponse, path: string | null = null) {
      this.resetTransientDocumentWork();
      this.data = response.fileData;
      this.currentFilePath = path !== null ? path : response.fileData.path || null;
      this.documentId = response.editorSession.documentId;
      this.revision = response.editorSession.revision;
      this.clampSelectionToCurrentSheet();
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
      this.resetTransientDocumentWork();
      this.data = null;
      this.currentFilePath = null;
      this.documentId = null;
      this.revision = 0;
      this.lifecycle = "idle";
      resolveLifecycleIdleWaiters(this);
      this.resetSessionUi();
      useDocumentStatusStore().reset();
    },
    resetTransientDocumentWork() {
      this.resetMutationQueue();
      usePendingCellSavesStore().reset();
      useDocumentStatusStore().clearPendingContentChange();
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
        this.applyResponseStatus(response);
        useSearchSessionStore().clearSearch();
        return { data: this.data, resyncRequired: true };
      }
      if (response.revision === this.revision && response.patches?.length) {
        this.applyResponseStatus(response);
        if (mutationInvalidatesSearch(response.patches)) {
          useSearchSessionStore().clearSearch();
        }
        return { data: this.data, resyncRequired: true };
      }
      if (response.revision === this.revision) {
        this.applyResponseStatus(response);
        return { data: this.data, resyncRequired: false };
      }
      this.revision = response.revision;
      const result = applyDocumentPatches(this.data, response.patches);
      this.data = result.data;
      useEditorSelectionStore().applyEditorPatches(response.patches);
      if (mutationInvalidatesSearch(response.patches)) {
        useSearchSessionStore().clearSearch();
      }
      this.applyResponseStatus(response);
      this.clampSelectionToCurrentSheet();
      return {
        data: result.data,
        resyncRequired: result.resyncRequired,
      };
    },
    async applyMutationResponseWithResync(
      response: EditorMutationResponse,
      fetchProjection: (context: EditorCommandContext) => Promise<FileData>
    ): Promise<MutationApplyResult> {
      const snapshot = this.captureMutationSnapshot();
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
        this.replaceProjection(projection);
      } catch (error) {
        if (this.matchesCommandContext(resyncContext)) {
          this.restoreMutationSnapshot(snapshot);
        }
        throw error;
      }
      return {
        data: this.data,
        resyncRequired: true,
      };
    },
    replaceProjection(data: FileData) {
      const currentFileName = this.data?.fileName;
      this.data = {
        ...data,
        path: this.currentFilePath ?? data.path,
        fileName: currentFileName ?? data.fileName,
      };
      this.clampSelectionToCurrentSheet();
    },
    applyResponseStatus(response: EditorMutationResponse) {
      useDocumentStatusStore().formulaStatus = response.formulaStatus;
      useDocumentStatusStore().capabilities = response.capabilities;
      useDocumentStatusStore().applyEditorState(response.editorState);
    },
    captureMutationSnapshot(): DocumentSessionSnapshot {
      const statusStore = useDocumentStatusStore();
      const selectionStore = useEditorSelectionStore();
      return {
        data: this.data,
        currentFilePath: this.currentFilePath,
        documentId: this.documentId,
        revision: this.revision,
        lifecycle: this.lifecycle,
        status: {
          canUndo: statusStore.canUndo,
          canRedo: statusStore.canRedo,
          isContentDirty: statusStore.isContentDirty,
          hasPendingContentChange: statusStore.hasPendingContentChange,
          formulaStatus: statusStore.formulaStatus,
          capabilities: statusStore.capabilities,
          history: statusStore.history,
        },
        selection: {
          currentSheetIndex: selectionStore.currentSheetIndex,
          selectedCell: cloneCellPosition(selectionStore.selectedCell),
          cellEditorValue: selectionStore.cellEditorValue,
          autoScroll: selectionStore.autoScroll,
          sheetSelectedCells: cloneSelectedCells(selectionStore.sheetSelectedCells),
        },
      };
    },
    restoreMutationSnapshot(snapshot: DocumentSessionSnapshot) {
      this.data = snapshot.data;
      this.currentFilePath = snapshot.currentFilePath;
      this.documentId = snapshot.documentId;
      this.revision = snapshot.revision;
      this.lifecycle = snapshot.lifecycle;

      const statusStore = useDocumentStatusStore();
      statusStore.canUndo = snapshot.status.canUndo;
      statusStore.canRedo = snapshot.status.canRedo;
      statusStore.isContentDirty = snapshot.status.isContentDirty;
      statusStore.hasPendingContentChange = snapshot.status.hasPendingContentChange;
      statusStore.formulaStatus = snapshot.status.formulaStatus;
      statusStore.capabilities = snapshot.status.capabilities;
      statusStore.history = snapshot.status.history;

      const selectionStore = useEditorSelectionStore();
      selectionStore.currentSheetIndex = snapshot.selection.currentSheetIndex;
      selectionStore.selectedCell = cloneCellPosition(snapshot.selection.selectedCell);
      selectionStore.cellEditorValue = snapshot.selection.cellEditorValue;
      selectionStore.autoScroll = snapshot.selection.autoScroll;
      selectionStore.sheetSelectedCells = cloneSelectedCells(snapshot.selection.sheetSelectedCells);
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

      const snapshot = this.captureMutationSnapshot();
      try {
        const [projection, session] = await Promise.all([
          fetchProjection(context),
          fetchEditorSession(context),
        ]);
        if (!this.matchesCommandContext(context)) {
          return;
        }
        this.replaceProjection(projection);
        this.applyEditorSessionForContext(context, session);
      } catch (error) {
        if (this.matchesCommandContext(context)) {
          this.restoreMutationSnapshot(snapshot);
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
      this.documentId = info.documentId;
      this.revision = Math.max(this.revision, info.revision);
      useDocumentStatusStore().applyEditorSession(info);
    },
    resetSessionUi() {
      useEditorSelectionStore().reset();
      useSearchSessionStore().reset();
      usePendingCellSavesStore().reset();
    },
    clampSelectionToCurrentSheet() {
      const selectionStore = useEditorSelectionStore();
      if (!this.data) {
        selectionStore.clearSelection();
        return;
      }
      selectionStore.clampToSheetData(this.data.sheets.length, (sheetIndex, row, col) => {
        const sheet = this.data?.sheets[sheetIndex];
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
    },
  },
});

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

function cloneCellPosition(cell: CellPosition | null): CellPosition | null {
  return cell ? { ...cell } : null;
}

function cloneSelectedCells(cells: Map<number, CellPosition>): Map<number, CellPosition> {
  return new Map(Array.from(cells, ([sheetIndex, cell]) => [sheetIndex, { ...cell }]));
}

function mutationInvalidatesSearch(patches: EditorPatch[] | undefined): boolean {
  return (patches ?? []).some((patch) => patch.type !== "Layout");
}
