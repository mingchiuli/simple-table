import type {
  EditorMutationResponse,
  EditorSessionInfo,
  FileData,
  OpenDocumentResponse,
} from "@/types";
import { applyDocumentPatches } from "@/stores/documentPatches";
import { usePendingCellSavesStore } from "@/stores/pendingCellSaves";
import { useSearchSessionStore } from "@/stores/searchSession";
import { useDocumentStatusStore } from "@/stores/documentStatus";
import { resetEditorMutationQueue } from "@/composables/useEditorMutationQueue";

type CellPosition = { row: number; col: number };

export type MutationApplyResult = {
  data: FileData | null;
  resyncRequired: boolean;
};

export const useDocumentSessionStore = defineStore("documentSession", {
  state: () => ({
    data: null as FileData | null,
    currentFilePath: null as string | null,
    documentId: null as number | null,
    revision: 0,
    mutationScope: 0,

    currentSheetIndex: 0,
    selectedCell: null as CellPosition | null,
    cellEditorValue: "",
    autoScroll: false,
    sheetSelectedCells: new Map<number, CellPosition>(),
  }),
  actions: {
    openDocument(data: FileData, path: string | null = null) {
      resetEditorMutationQueue(this.mutationScope);
      this.mutationScope += 1;
      this.data = data;
      this.currentFilePath = path;
      this.documentId = null;
      this.revision = 0;
      this.resetUiForCurrentDocument();
    },
    openDocumentResponse(response: OpenDocumentResponse, path: string | null = null) {
      resetEditorMutationQueue(this.mutationScope);
      this.mutationScope += 1;
      this.data = response.fileData;
      this.currentFilePath = path !== null ? path : response.fileData.path || null;
      this.documentId = response.editorSession.documentId;
      this.revision = response.editorSession.revision;
      this.resetUiForCurrentDocument();
      const statusStore = useDocumentStatusStore();
      statusStore.reset();
      statusStore.applyEditorSession(response.editorSession);
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
      this.resetUiForCurrentDocument();
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
    resetUiForCurrentDocument() {
      this.currentSheetIndex = 0;
      this.selectedCell = null;
      this.cellEditorValue = "";
      this.autoScroll = false;
      this.sheetSelectedCells = new Map();
      useSearchSessionStore().reset();
      usePendingCellSavesStore().reset();
    },
    selectCell(row: number, col: number, autoScroll = false) {
      this.autoScroll = autoScroll;
      this.selectedCell = { row, col };
    },
    clearSelection() {
      this.selectedCell = null;
      this.cellEditorValue = "";
    },
    rememberCurrentSheetSelection() {
      if (this.selectedCell) {
        this.sheetSelectedCells.set(this.currentSheetIndex, this.selectedCell);
      }
    },
    restoreSheetSelection(sheetIndex: number, editorValueFor: (cell: CellPosition) => string) {
      this.currentSheetIndex = sheetIndex;
      const savedCell = this.sheetSelectedCells.get(sheetIndex);
      if (!savedCell) {
        this.clearSelection();
        return;
      }
      this.selectedCell = savedCell;
      this.cellEditorValue = editorValueFor(savedCell);
      this.autoScroll = true;
    },
    clampSelectionToCurrentSheet() {
      if (!this.data) {
        this.clearSelection();
        return;
      }
      if (this.currentSheetIndex >= this.data.sheets.length) {
        this.currentSheetIndex = Math.max(0, this.data.sheets.length - 1);
      }
      if (!this.selectedCell) return;

      const sheet = this.data.sheets[this.currentSheetIndex];
      const row = sheet?.rows[this.selectedCell.row];
      if (!row || this.selectedCell.col >= row.length) {
        this.clearSelection();
      }
    },
  },
});
