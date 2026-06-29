import type {
  CellValue,
  EditorMutationResponse,
  EditorPatch,
  EditorStateInfo,
  FileData,
  SearchResult,
  SheetCellChange,
} from "@/types";

type CellPosition = { row: number; col: number };

export type PendingCellChange = {
  sheetIndex: number;
  row: number;
  col: number;
  value: string;
  oldValue: CellValue;
};

export const useDocumentSessionStore = defineStore("documentSession", {
  state: () => ({
    data: null as FileData | null,
    currentFilePath: null as string | null,

    currentSheetIndex: 0,
    selectedCell: null as CellPosition | null,
    cellEditorValue: "",
    autoScroll: false,
    searchResults: [] as SearchResult[],
    searchQuery: "",
    isSearching: false,
    sheetSelectedCells: new Map<number, CellPosition>(),
    draftCellValues: new Map<string, string>(),
    pendingCellChanges: new Map<string, PendingCellChange>(),
    inFlightCellChanges: new Map<string, PendingCellChange>(),
    sheetColumnWidths: {} as Record<number, Record<number, number>>,
    sheetRowHeights: {} as Record<number, Record<number, number>>,
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
      this.resetUiForCurrentDocument();
    },
    applyMutationResponse(response: EditorMutationResponse): FileData | null {
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
        if (patch.type === "FullSnapshot") {
          nextData = this.applySnapshot(patch.data.fileData);
        } else if (patch.type === "Cells") {
          nextData = this.applyCellChanges(patch.data.changes);
        } else if (patch.type === "Layout") {
          nextData = this.applyLayoutPatch(
            patch.data.patch.sheetIndex,
            patch.data.patch.columnWidths ?? {},
            patch.data.patch.rowHeights ?? {}
          );
        }
      }
      this.syncLayoutFromData();
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
      this.searchResults = [];
      this.searchQuery = "";
      this.isSearching = false;
      this.sheetSelectedCells = new Map();
      this.resetPendingEdits();
      this.syncLayoutFromData();
    },
    resetPendingEdits() {
      this.draftCellValues.clear();
      this.pendingCellChanges.clear();
      this.inFlightCellChanges.clear();
    },
    syncLayoutFromData() {
      if (!this.data) {
        this.sheetColumnWidths = {};
        this.sheetRowHeights = {};
        return;
      }

      this.sheetColumnWidths = Object.fromEntries(
        this.data.sheets
          .map((sheet, index) => [index, sheet.columnWidths ?? {}] as const)
          .filter(([, widths]) => Object.keys(widths).length > 0)
      );
      this.sheetRowHeights = Object.fromEntries(
        this.data.sheets
          .map((sheet, index) => [index, sheet.rowHeights ?? {}] as const)
          .filter(([, heights]) => Object.keys(heights).length > 0)
      );
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
    setColumnWidth(sheetIndex: number, colIndex: number, width: number | undefined) {
      this.sheetColumnWidths = patchNestedNumberRecord(this.sheetColumnWidths, sheetIndex, colIndex, width);
    },
    setRowHeight(sheetIndex: number, rowIndex: number, height: number | undefined) {
      this.sheetRowHeights = patchNestedNumberRecord(this.sheetRowHeights, sheetIndex, rowIndex, height);
    },
    clearSearch() {
      this.searchResults = [];
      this.searchQuery = "";
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

function patchNestedNumberRecord(
  current: Record<number, Record<number, number>>,
  sheetIndex: number,
  key: number,
  value: number | undefined
): Record<number, Record<number, number>> {
  const sheetRecord = { ...(current[sheetIndex] ?? {}) };
  if (value === undefined) {
    delete sheetRecord[key];
  } else {
    sheetRecord[key] = value;
  }

  const next = { ...current };
  if (Object.keys(sheetRecord).length) {
    next[sheetIndex] = sheetRecord;
  } else {
    delete next[sheetIndex];
  }
  return next;
}
