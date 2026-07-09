import type {
  EditorMutationResponse,
  EditorSessionInfo,
  FileData,
  FormulaStatus,
  HistoryStatus,
  OpenDocumentResponse,
  SavedDocumentResponse,
  WorkbookCapabilities,
} from "@/types";
import { applyDocumentPatches } from "@/stores/documentPatches";
import { usePendingCellSavesStore } from "@/stores/pendingCellSaves";
import { useSearchSessionStore } from "@/stores/searchSession";
import { useDocumentStatusStore } from "@/stores/documentStatus";
import { useEditorSelectionStore } from "@/stores/editorSelection";

export type MutationApplyResult = {
  data: FileData | null;
  resyncRequired: boolean;
};

export type DocumentSessionLifecycle = "idle" | "loading" | "saving";

type MutationQueueRuntime = {
  tail: Promise<void> | null;
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

const mutationQueueRuntimes = new WeakMap<object, MutationQueueRuntime>();

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
    beginLifecycle(lifecycle: Exclude<DocumentSessionLifecycle, "idle">) {
      this.lifecycle = lifecycle;
    },
    endLifecycle(lifecycle: Exclude<DocumentSessionLifecycle, "idle">) {
      if (this.lifecycle === lifecycle) {
        this.lifecycle = "idle";
      }
    },
    resetMutationQueue() {
      mutationRuntimeFor(this).tail = null;
    },
    enqueueMutation<T>(task: () => Promise<T>): Promise<T> {
      const runtime = mutationRuntimeFor(this);
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
    waitForMutations(): Promise<void> {
      return mutationRuntimeFor(this).tail ?? Promise.resolve();
    },
    openDocument(data: FileData, path: string | null = null) {
      this.resetMutationQueue();
      this.data = data;
      this.currentFilePath = path;
      this.documentId = null;
      this.revision = 0;
      this.resetSessionUi();
    },
    openDocumentResponse(response: OpenDocumentResponse, path: string | null = null) {
      this.resetMutationQueue();
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
      this.data = response.fileData;
      this.currentFilePath = path !== null ? path : response.fileData.path || null;
      this.documentId = response.editorSession.documentId;
      this.revision = response.editorSession.revision;
      this.clampSelectionToCurrentSheet();
      usePendingCellSavesStore().reset();
      useDocumentStatusStore().clearPendingContentChange();
      useDocumentStatusStore().applyEditorSession(response.editorSession);
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
      this.resetMutationQueue();
      this.data = null;
      this.currentFilePath = null;
      this.documentId = null;
      this.revision = 0;
      this.lifecycle = "idle";
      this.resetSessionUi();
    },
    applyMutationResponse(response: EditorMutationResponse): MutationApplyResult {
      if (response.protocolVersion !== 1) {
        throw new Error(`Unsupported editor mutation protocol: ${response.protocolVersion}`);
      }
      if (this.documentId !== null && response.documentId !== this.documentId) {
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
        return { data: this.data, resyncRequired: true };
      }
      if (response.revision === this.revision && response.patches?.length) {
        this.applyResponseStatus(response);
        return { data: this.data, resyncRequired: true };
      }
      if (response.revision === this.revision) {
        this.applyResponseStatus(response);
        return { data: this.data, resyncRequired: false };
      }
      this.revision = response.revision;
      const result = applyDocumentPatches(this.data, response.patches);
      this.data = result.data;
      this.applyResponseStatus(response);
      this.clampSelectionToCurrentSheet();
      return {
        data: result.data,
        resyncRequired: result.resyncRequired,
      };
    },
    async applyMutationResponseWithResync(
      response: EditorMutationResponse,
      fetchProjection: () => Promise<FileData>
    ): Promise<MutationApplyResult> {
      const snapshot = this.captureMutationSnapshot();
      const result = this.applyMutationResponse(response);
      if (!result.resyncRequired) {
        return result;
      }
      try {
        this.replaceProjection(await fetchProjection());
      } catch (error) {
        this.restoreMutationSnapshot(snapshot);
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
      fetchEditorSession: () => Promise<EditorSessionInfo | null | undefined>,
      fetchProjection?: () => Promise<FileData>
    ) {
      if (!fetchProjection) {
        this.applyEditorSession(await fetchEditorSession());
        return;
      }

      const [projection, session] = await Promise.all([
        fetchProjection(),
        fetchEditorSession(),
      ]);
      this.replaceProjection(projection);
      this.applyEditorSession(session);
    },
    applyEditorSession(info: EditorSessionInfo | null | undefined) {
      if (!info) {
        useDocumentStatusStore().applyEditorSession(null);
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
      selectionStore.clampToSheetData(this.data.sheets.length, (sheetIndex, row) =>
        this.data?.sheets[sheetIndex]?.rows[row]?.length ?? null
      );
    },
  },
});

function mutationRuntimeFor(store: object): MutationQueueRuntime {
  let runtime = mutationQueueRuntimes.get(store);
  if (!runtime) {
    runtime = { tail: null };
    mutationQueueRuntimes.set(store, runtime);
  }
  return runtime;
}

function cloneCellPosition(cell: CellPosition | null): CellPosition | null {
  return cell ? { ...cell } : null;
}

function cloneSelectedCells(cells: Map<number, CellPosition>): Map<number, CellPosition> {
  return new Map(Array.from(cells, ([sheetIndex, cell]) => [sheetIndex, { ...cell }]));
}
