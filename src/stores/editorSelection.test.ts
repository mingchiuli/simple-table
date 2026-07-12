import { beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useEditorSelectionStore } from "@/stores/editorSelection";
import type { EditorPatch } from "@/types";

function rowDeleted(sheetIndex: number, rowIndex: number, count: number): EditorPatch {
  return {
    type: "RowDeleted",
    data: {
      patch: {
        sheetIndex,
        rowIndex,
        count,
      },
    },
  };
}

function sheetInserted(sheetIndex: number): EditorPatch {
  return {
    type: "SheetInserted",
    data: {
      patch: {
        sheetIndex,
        sheet: {
          name: "Inserted",
          extent: { rowCount: 0, columnCount: 0 },
          layout: { columnWidths: {}, rowHeights: {} },
        },
      },
    },
  };
}

function sheetsReplaced(startIndex: number): EditorPatch {
  return {
    type: "SheetsReplaced",
    data: {
      patch: {
        startIndex,
        sheets: [{
          name: "Replacement",
          extent: { rowCount: 0, columnCount: 0 },
          layout: { columnWidths: {}, rowHeights: {} },
        }],
      },
    },
  };
}

describe("editorSelection store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("moves remembered sheet selections with row structure patches", () => {
    const store = useEditorSelectionStore();
    store.sheetSelectedCells.set(1, { row: 4, col: 2 });

    store.applyEditorPatches([rowDeleted(1, 1, 2)]);

    expect(store.sheetSelectedCells.get(1)).toEqual({ row: 2, col: 2 });
  });

  it("switches sheets through the selection boundary", () => {
    const store = useEditorSelectionStore();
    store.selectCell(2, 3);
    store.switchSheet(1, () => "restored");

    expect(store.currentSheetIndex).toBe(1);
    expect(store.selectedCell).toBeNull();
    expect(store.sheetSelectedCells.get(0)).toEqual({ row: 2, col: 3 });

    store.sheetSelectedCells.set(2, { row: 4, col: 5 });
    store.switchSheet(2, () => "B5");

    expect(store.currentSheetIndex).toBe(2);
    expect(store.selectedCell).toEqual({ row: 4, col: 5 });
    expect(store.cellEditorValue).toBe("B5");
    expect(store.autoScroll).toBe(true);
  });

  it("focuses a search result as one selection update", () => {
    const store = useEditorSelectionStore();

    store.focusSearchResult(2, 4, 5, "found");

    expect(store.currentSheetIndex).toBe(2);
    expect(store.selectedCell).toEqual({ row: 4, col: 5 });
    expect(store.cellEditorValue).toBe("found");
    expect(store.autoScroll).toBe(true);
  });

  it("remaps remembered sheet selections when a sheet is inserted", () => {
    const store = useEditorSelectionStore();
    store.currentSheetIndex = 2;
    store.sheetSelectedCells.set(1, { row: 0, col: 0 });
    store.sheetSelectedCells.set(2, { row: 1, col: 1 });

    store.applyEditorPatches([sheetInserted(1)]);

    expect(store.currentSheetIndex).toBe(3);
    expect(store.sheetSelectedCells.get(2)).toEqual({ row: 0, col: 0 });
    expect(store.sheetSelectedCells.get(3)).toEqual({ row: 1, col: 1 });
  });

  it("clears selections inside a replaced sheet range", () => {
    const store = useEditorSelectionStore();
    store.currentSheetIndex = 2;
    store.selectCell(1, 1);
    store.sheetSelectedCells.set(0, { row: 0, col: 0 });
    store.sheetSelectedCells.set(2, { row: 1, col: 1 });

    store.applyEditorPatches([sheetsReplaced(1)]);

    expect(store.selectedCell).toBeNull();
    expect(store.sheetSelectedCells.get(0)).toEqual({ row: 0, col: 0 });
    expect(store.sheetSelectedCells.has(2)).toBe(false);
  });

  it("clamps remembered selections to current sheet bounds", () => {
    const store = useEditorSelectionStore();
    store.sheetSelectedCells.set(0, { row: 0, col: 0 });
    store.sheetSelectedCells.set(1, { row: 4, col: 4 });
    store.sheetSelectedCells.set(3, { row: 0, col: 0 });

    store.clampToSheetData(2, (sheetIndex, row, col) => (
      sheetIndex === 0 && row === 0 && col === 0
    ));

    expect(store.sheetSelectedCells.get(0)).toEqual({ row: 0, col: 0 });
    expect(store.sheetSelectedCells.has(1)).toBe(false);
    expect(store.sheetSelectedCells.has(3)).toBe(false);
  });
});
