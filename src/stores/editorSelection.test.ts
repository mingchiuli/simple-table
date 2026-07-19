import { beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useEditorSelectionStore } from "@/stores/editorSelection";
import type { SelectionTransform } from '@/types/editorRuntime';

function rowDeleted(sheetIndex: number, rowIndex: number, count: number): SelectionTransform {
  return { type: 'rowDeleted', sheetIndex, rowIndex, count };
}

function sheetInserted(sheetIndex: number): SelectionTransform {
  return { type: 'sheetInserted', sheetIndex };
}

function sheetsReplaced(startIndex: number): SelectionTransform {
  return { type: 'sheetsReplaced', startIndex };
}

describe("editorSelection store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("moves remembered sheet selections with row structure patches", () => {
    const store = useEditorSelectionStore();
    store.sheetSelectedCells[1] = { row: 4, col: 2 };

    store.applySelectionTransforms([rowDeleted(1, 1, 2)]);

    expect(store.sheetSelectedCells[1]).toEqual({ row: 2, col: 2 });
  });

  it("switches sheets through the selection boundary", () => {
    const store = useEditorSelectionStore();
    store.selectCell(2, 3);
    store.switchSheet(1, () => "restored");

    expect(store.currentSheetIndex).toBe(1);
    expect(store.selectedCell).toBeNull();
    expect(store.sheetSelectedCells[0]).toEqual({ row: 2, col: 3 });

    store.sheetSelectedCells[2] = { row: 4, col: 5 };
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
    store.sheetSelectedCells[1] = { row: 0, col: 0 };
    store.sheetSelectedCells[2] = { row: 1, col: 1 };

    store.applySelectionTransforms([sheetInserted(1)]);

    expect(store.currentSheetIndex).toBe(3);
    expect(store.sheetSelectedCells[2]).toEqual({ row: 0, col: 0 });
    expect(store.sheetSelectedCells[3]).toEqual({ row: 1, col: 1 });
  });

  it("clears selections inside a replaced sheet range", () => {
    const store = useEditorSelectionStore();
    store.currentSheetIndex = 2;
    store.selectCell(1, 1);
    store.sheetSelectedCells[0] = { row: 0, col: 0 };
    store.sheetSelectedCells[2] = { row: 1, col: 1 };

    store.applySelectionTransforms([sheetsReplaced(1)]);

    expect(store.selectedCell).toBeNull();
    expect(store.sheetSelectedCells[0]).toEqual({ row: 0, col: 0 });
    expect(store.sheetSelectedCells[2]).toBeUndefined();
  });

  it("clamps remembered selections to current sheet bounds", () => {
    const store = useEditorSelectionStore();
    store.sheetSelectedCells[0] = { row: 0, col: 0 };
    store.sheetSelectedCells[1] = { row: 4, col: 4 };
    store.sheetSelectedCells[3] = { row: 0, col: 0 };

    store.clampToSheetData(2, (sheetIndex, row, col) => (
      sheetIndex === 0 && row === 0 && col === 0
    ));

    expect(store.sheetSelectedCells[0]).toEqual({ row: 0, col: 0 });
    expect(store.sheetSelectedCells[1]).toBeUndefined();
    expect(store.sheetSelectedCells[3]).toBeUndefined();
  });

  it('keeps selection state JSON serializable', () => {
    const store = useEditorSelectionStore();
    store.sheetSelectedCells[2] = { row: 4, col: 5 };

    expect(JSON.parse(JSON.stringify(store.$state)).sheetSelectedCells).toEqual({
      2: { row: 4, col: 5 },
    });
  });
});
