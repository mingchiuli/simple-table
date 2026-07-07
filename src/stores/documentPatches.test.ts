import { describe, expect, it } from "vitest";
import { applyDocumentPatches } from "@/stores/documentPatches";
import { defaultRichProjection, type CellValue, type FileData, type SheetData } from "@/types";

function text(value: string): CellValue {
  return { type: "cell", kind: "text", raw: value, display: value };
}

function sheet(name: string, rows: CellValue[][]): SheetData {
  return {
    name,
    rows,
    merges: [],
    rich: defaultRichProjection(),
  };
}

describe("applyDocumentPatches", () => {
  it("replaces a sheet with the backend authoritative projection", () => {
    const data: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("old")]])],
    };
    const replacement: SheetData = {
      name: "Sheet1",
      rows: [[text("new"), text("value")]],
      merges: [{ startRow: 0, startCol: 0, endRow: 0, endCol: 1 }],
      columnWidths: { 1: 180 },
      rowHeights: { 0: 96 },
      rich: {
        ...defaultRichProjection(),
        cellStyles: { A1: { bold: true } },
        drawings: [{ kind: "image", fromRow: 0, fromCol: 0, toRow: 1, toCol: 1 }],
      },
    };

    const result = applyDocumentPatches(data, [
      { type: "SheetUpdated", data: { patch: { sheetIndex: 0, sheet: replacement } } },
    ]);

    expect(result.resyncRequired).toBe(false);
    expect(result.data?.sheets[0]).toEqual(replacement);
  });

  it("applies cells and layout without replacing unrelated sheet metadata", () => {
    const data: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [{
        ...sheet("Sheet1", [[text("old")]]),
        merges: [{ startRow: 0, startCol: 0, endRow: 0, endCol: 0 }],
      }],
    };

    const result = applyDocumentPatches(data, [
      { type: "Cells", data: { changes: [{ sheetIndex: 0, row: 0, col: 1, value: text("new") }] } },
      { type: "Layout", data: { patch: { sheetIndex: 0, columnWidths: { 1: 160 }, rowHeights: { 0: 90 } } } },
    ]);

    expect(result.data?.sheets[0].rows[0][1]).toEqual(text("new"));
    expect(result.data?.sheets[0].columnWidths?.[1]).toBe(160);
    expect(result.data?.sheets[0].rowHeights?.[0]).toBe(90);
    expect(result.data?.sheets[0].merges).toEqual(data.sheets[0].merges);
  });

  it("applies row structure patches without replacing the whole sheet", () => {
    const data: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("A1")], [text("A2")]])],
    };

    const result = applyDocumentPatches(data, [
      {
        type: "RowInserted",
        data: {
          patch: {
            sheetIndex: 0,
            rowIndex: 1,
            rows: [[text("inserted")]],
            metadata: {
              merges: [{ startRow: 1, startCol: 0, endRow: 1, endCol: 1 }],
              rowHeights: { 1: 88 },
              columnWidths: undefined,
              rich: {
                ...defaultRichProjection(),
                cellStyles: { A2: { bold: true } },
              },
            },
          },
        },
      },
    ]);

    expect(result.resyncRequired).toBe(false);
    expect(result.data?.sheets[0].rows).toEqual([[text("A1")], [text("inserted")], [text("A2")]]);
    expect(result.data?.sheets[0].merges).toEqual([{ startRow: 1, startCol: 0, endRow: 1, endCol: 1 }]);
    expect(result.data?.sheets[0].rowHeights?.[1]).toBe(88);
    expect(result.data?.sheets[0].rich.cellStyles?.A2).toEqual({ bold: true });
  });

  it("applies column structure patches without replacing the whole sheet", () => {
    const data: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("A1"), text("B1")], [text("A2"), text("B2")]])],
    };

    const result = applyDocumentPatches(data, [
      {
        type: "ColumnDeleted",
        data: {
          patch: {
            sheetIndex: 0,
            colIndex: 0,
            count: 1,
            metadata: {
              merges: [],
              columnWidths: { 0: 144 },
              rowHeights: undefined,
              rich: {
                ...defaultRichProjection(),
                cellStyles: { A1: { italic: true } },
              },
            },
          },
        },
      },
    ]);

    expect(result.resyncRequired).toBe(false);
    expect(result.data?.sheets[0].rows).toEqual([[text("B1")], [text("B2")]]);
    expect(result.data?.sheets[0].columnWidths?.[0]).toBe(144);
    expect(result.data?.sheets[0].rich.cellStyles?.A1).toEqual({ italic: true });
  });

});
