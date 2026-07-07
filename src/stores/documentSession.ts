import type {
  EditorMutationResponse,
  EditorSessionInfo,
  FileData,
  OpenDocumentResponse,
  SavedDocumentResponse,
} from "@/types";
import { applyDocumentPatches } from "@/stores/documentPatches";
import { usePendingCellSavesStore } from "@/stores/pendingCellSaves";
import { useSearchSessionStore } from "@/stores/searchSession";
import { useDocumentStatusStore } from "@/stores/documentStatus";
import { useEditorSelectionStore } from "@/stores/editorSelection";
import { resetEditorMutationQueue } from "@/composables/useEditorMutationQueue";

export type MutationApplyResult = {
  data: FileData | null;
  resyncRequired: boolean;
};

export type DocumentSessionLifecycle = "idle" | "loading" | "saving";

export const useDocumentSessionStore = defineStore("documentSession", {
  state: () => ({
    data: null as FileData | null,
    currentFilePath: null as string | null,
    documentId: null as number | null,
    revision: 0,
    mutationScope: 0,
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
    openDocument(data: FileData, path: string | null = null) {
      resetEditorMutationQueue(this.mutationScope);
      this.mutationScope += 1;
      this.data = data;
      this.currentFilePath = path;
      this.documentId = null;
      this.revision = 0;
      this.resetSessionUi();
    },
    openDocumentResponse(response: OpenDocumentResponse, path: string | null = null) {
      resetEditorMutationQueue(this.mutationScope);
      this.mutationScope += 1;
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
      resetEditorMutationQueue(this.mutationScope);
      this.mutationScope += 1;
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
