import type {
  EditorMutationResponse,
  EditorSessionInfo,
  FileData,
} from "@/types";
import { applyDocumentPatches } from "@/stores/documentPatches";
import { usePendingCellSavesStore } from "@/stores/pendingCellSaves";
import { useSearchSessionStore } from "@/stores/searchSession";
import { useDocumentStatusStore } from "@/stores/documentStatus";

type CellPosition = { row: number; col: number };

export const useDocumentSessionStore = defineStore("documentSession", {
  state: () => ({
    data: null as FileData | null,
    currentFilePath: null as string | null,
    documentId: null as number | null,
    revision: 0,

    currentSheetIndex: 0,
    selectedCell: null as CellPosition | null,
    cellEditorValue: "",
    autoScroll: false,
    sheetSelectedCells: new Map<number, CellPosition>(),
  }),
  actions: {
    openDocument(data: FileData, path: string | null = null) {
      this.data = data;
      this.currentFilePath = path;
      this.documentId = null;
      this.revision = 0;
      this.resetUiForCurrentDocument();
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
      this.data = null;
      this.currentFilePath = null;
      this.documentId = null;
      this.revision = 0;
      this.resetUiForCurrentDocument();
    },
    applyMutationResponse(response: EditorMutationResponse): FileData | null {
      if (response.protocolVersion !== 1) {
        throw new Error(`Unsupported editor mutation protocol: ${response.protocolVersion}`);
      }
      if (this.documentId !== null && response.documentId !== this.documentId) {
        return this.data;
      }
      if (this.documentId === null) {
        this.documentId = response.documentId;
      }
      if (response.revision < this.revision) {
        return this.data;
      }
      if (response.revision === this.revision && response.patches?.length) {
        throw new Error(`Duplicate editor mutation revision with patches: ${response.revision}`);
      }
      if (response.revision === this.revision) {
        this.applyResponseStatus(response);
        return this.data;
      }
      this.revision = response.revision;
      const nextData = applyDocumentPatches(this.data, response.patches);
      this.data = nextData;
      this.applyResponseStatus(response);
      this.clampSelectionToCurrentSheet();
      return nextData;
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
