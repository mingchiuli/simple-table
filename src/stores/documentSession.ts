import type {
  CellValue,
  EditorMutationResponse,
  EditorPatch,
  EditorSessionInfo,
  EditorStateInfo,
  FileData,
  FormulaStatus,
  SheetCellChange,
} from "@/types";
import { usePendingCellSavesStore } from "@/stores/pendingCellSaves";
import { useSearchSessionStore } from "@/stores/searchSession";
import { useSheetLayoutStore } from "@/stores/sheetLayout";

type CellPosition = { row: number; col: number };

export const useDocumentSessionStore = defineStore("documentSession", {
  state: () => ({
    data: null as FileData | null,
    currentFilePath: null as string | null,
    documentId: null as number | null,
    revision: 0,
    formulaStatus: { state: "ready" } as FormulaStatus,

    currentSheetIndex: 0,
    selectedCell: null as CellPosition | null,
    cellEditorValue: "",
    autoScroll: false,
    sheetSelectedCells: new Map<number, CellPosition>(),
    canUndo: false,
    canRedo: false,
    isContentDirty: false,
    hasPendingContentChange: false,
  }),
  getters: {
    hasUnsavedChanges: (state) => state.isContentDirty || state.hasPendingContentChange,
  },
  actions: {
    openDocument(data: FileData, path: string | null = null) {
      this.data = data;
      this.currentFilePath = path;
      this.documentId = null;
      this.revision = 0;
      this.formulaStatus = { state: "ready" };
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
      this.formulaStatus = { state: "ready" };
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
      this.revision = response.revision;
      this.formulaStatus = response.formulaStatus;
      const nextData = this.applyPatches(response.patches);
      this.applyEditorState(response.editorState);
      this.clampSelectionToCurrentSheet();
      return nextData;
    },
    applyEditorState(state: EditorStateInfo | null | undefined) {
      this.canUndo = state?.canUndo ?? false;
      this.canRedo = state?.canRedo ?? false;
      this.isContentDirty = state?.isDirty ?? false;
    },
    applyEditorSession(info: EditorSessionInfo | null | undefined) {
      if (!info) {
        this.applyEditorState(null);
        return;
      }
      if (this.documentId !== null && info.documentId !== this.documentId) {
        return;
      }
      this.documentId = info.documentId;
      this.revision = Math.max(this.revision, info.revision);
      this.formulaStatus = info.formulaStatus;
      this.applyEditorState(info.editorState);
    },
    resetDocumentStatus() {
      this.canUndo = false;
      this.canRedo = false;
      this.isContentDirty = false;
      this.hasPendingContentChange = false;
    },
    markPendingContentChange() {
      this.hasPendingContentChange = true;
    },
    clearPendingContentChange() {
      this.hasPendingContentChange = false;
    },
    applyPatches(patches: EditorPatch[] | undefined): FileData | null {
      let nextData = this.data;
      for (const patch of patches ?? []) {
        switch (patch.type) {
          case "FullSnapshot":
            nextData = this.applySnapshot(patch.data.fileData);
            break;
          case "Cells":
            nextData = this.applyCellChanges(patch.data.changes);
            break;
          case "Layout":
            nextData = this.applyLayoutPatch(
              patch.data.patch.sheetIndex,
              patch.data.patch.columnWidths ?? {},
              patch.data.patch.rowHeights ?? {}
            );
            break;
          case "SheetSnapshot":
            nextData = this.applySheetSnapshot(patch.data.sheetIndex, patch.data.sheet);
            break;
          default:
            assertNever(patch);
        }
      }
      useSheetLayoutStore().syncFromData(this.data);
      return nextData;
    },
    applySnapshot(snapshot: FileData): FileData {
      const nextData = {
        ...snapshot,
        path: this.data?.path ?? snapshot.path,
        fileName: this.data?.fileName ?? snapshot.fileName,
      };
      this.data = nextData;
      return nextData;
    },
    applySheetSnapshot(sheetIndex: number, sheetSnapshot: FileData["sheets"][number]): FileData | null {
      if (!this.data || !this.data.sheets[sheetIndex]) return this.data;
      const nextData = {
        ...this.data,
        sheets: [...this.data.sheets],
      };
      nextData.sheets[sheetIndex] = sheetSnapshot;
      this.data = nextData;
      return nextData;
    },
    applyCellChanges(changes: SheetCellChange[]): FileData | null {
      if (!this.data) return null;
      if (!changes.length) return this.data;

      const nextData: FileData = {
        ...this.data,
        sheets: [...this.data.sheets],
      };
      const clonedRowsBySheet = new Map<number, SheetCellChange[]>();
      for (const change of changes) {
        const existing = clonedRowsBySheet.get(change.sheetIndex) ?? [];
        existing.push(change);
        clonedRowsBySheet.set(change.sheetIndex, existing);
      }

      for (const [sheetIndex, sheetChanges] of clonedRowsBySheet) {
        const sheet = this.data.sheets[sheetIndex];
        if (!sheet) continue;
        const rows = [...sheet.rows];
        nextData.sheets[sheetIndex] = { ...sheet, rows };
        for (const change of sheetChanges) {
          ensureCellExists(rows, change.row, change.col);
          rows[change.row][change.col] = change.value;
        }
      }

      this.data = nextData;
      return nextData;
    },
    applyLayoutPatch(
      sheetIndex: number,
      columnWidths: Record<number, number | null>,
      rowHeights: Record<number, number | null>
    ): FileData | null {
      const sheet = this.data?.sheets[sheetIndex];
      if (!this.data || !sheet) return this.data;

      const nextData = {
        ...this.data,
        sheets: [...this.data.sheets],
      };
      nextData.sheets[sheetIndex] = {
        ...sheet,
        columnWidths: patchNumberRecord(sheet.columnWidths, columnWidths),
        rowHeights: patchNumberRecord(sheet.rowHeights, rowHeights),
      };
      this.data = nextData;
      return nextData;
    },
    resetUiForCurrentDocument() {
      this.currentSheetIndex = 0;
      this.selectedCell = null;
      this.cellEditorValue = "";
      this.autoScroll = false;
      this.sheetSelectedCells = new Map();
      useSearchSessionStore().reset();
      usePendingCellSavesStore().reset();
      useSheetLayoutStore().syncFromData(this.data);
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

function assertNever(value: never): never {
  throw new Error(`Unhandled editor patch: ${JSON.stringify(value)}`);
}

function ensureCellExists(rows: CellValue[][], row: number, col: number) {
  while (rows.length <= row) {
    rows.push([]);
  }
  rows[row] = [...rows[row]];
  while (rows[row].length <= col) {
    rows[row].push(null);
  }
}

function patchNumberRecord(
  current: Record<number, number> | undefined,
  patch: Record<number, number | null>
): Record<number, number> | undefined {
  const next = { ...(current ?? {}) };
  for (const [key, value] of Object.entries(patch)) {
    if (value === null || value === undefined) {
      delete next[Number(key)];
    } else {
      next[Number(key)] = value;
    }
  }
  return Object.keys(next).length ? next : undefined;
}
