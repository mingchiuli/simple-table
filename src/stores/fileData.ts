import type { CellValue, EditorPatch, FileData, SheetCellChange } from "@/types";

export const useFileDataStore = defineStore("fileData", {
  state: () => ({
    data: null as FileData | null,
    // 当前打开文件的物理路径（来自 RecentFile.path），用于保存时定位原文件
    currentFilePath: null as string | null,
    documentVersion: 0,
  }),
  actions: {
    set(data: FileData, path: string | null = null) {
      this.data = data;
      this.currentFilePath = path;
      this.documentVersion += 1;
    },
    setData(data: FileData) {
      this.data = data;
    },
    applyPatches(patches: EditorPatch[] | undefined): FileData | null {
      let nextData = this.data;
      for (const patch of patches ?? []) {
        if (patch.type === 'FullSnapshot') {
          nextData = this.applySnapshot(patch.data.fileData);
        } else if (patch.type === 'Cells') {
          nextData = this.applyCellChanges(patch.data.changes);
        } else if (patch.type === 'Layout') {
          nextData = this.applyLayoutPatch(
            patch.data.patch.sheetIndex,
            patch.data.patch.columnWidths ?? {},
            patch.data.patch.rowHeights ?? {}
          );
        }
      }
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
    setPath(path: string | null) {
      this.currentFilePath = path;
    },
    clear() {
      this.data = null;
      this.currentFilePath = null;
      this.documentVersion += 1;
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
