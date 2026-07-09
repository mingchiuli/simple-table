import { describe, expect, it } from "vitest";
import { applyDocumentPatches } from "@/stores/documentPatches";
import { defaultRichProjection, type CellValue, type FileData, type SheetData } from "@/types";
import { blankCell } from "@/utils/cellValue";

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
      sheets: [{
        ...sheet("Sheet1", [[text("A1")], [text("A2")]]),
        rich: {
          ...defaultRichProjection(),
          cellStyles: { A1: { italic: true }, A2: { bold: true } },
        },
      }],
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
                scope: { type: "rows", start: 1 },
                projection: {
                  ...defaultRichProjection(),
                  cellStyles: { A2: { bold: true } },
                },
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
    expect(result.data?.sheets[0].rich.cellStyles?.A1).toEqual({ italic: true });
    expect(result.data?.sheets[0].rich.cellStyles?.A2).toEqual({ bold: true });
  });

  it("shifts row heights locally when row structure metadata omits dimensions", () => {
    const data: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [{
        ...sheet("Sheet1", [[text("A1")], [text("A2")], [text("A3")]]),
        rowHeights: { 0: 44, 2: 88 },
      }],
    };

    const inserted = applyDocumentPatches(data, [
      {
        type: "RowInserted",
        data: {
          patch: {
            sheetIndex: 0,
            rowIndex: 1,
            rows: [[text("inserted")]],
            metadata: {
              merges: [],
              rich: { scope: { type: "rows", start: 1 }, projection: defaultRichProjection() },
            },
          },
        },
      },
    ]);

    expect(inserted.data?.sheets[0].rowHeights).toEqual({ 0: 44, 3: 88 });

    const deleted = applyDocumentPatches(inserted.data, [
      {
        type: "RowDeleted",
        data: {
          patch: {
            sheetIndex: 0,
            rowIndex: 1,
            count: 1,
            metadata: {
              merges: [],
              rich: { scope: { type: "rows", start: 1 }, projection: defaultRichProjection() },
            },
          },
        },
      },
    ]);

    expect(deleted.data?.sheets[0].rowHeights).toEqual({ 0: 44, 2: 88 });
  });

  it("applies column structure patches without replacing the whole sheet", () => {
    const data: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [{
        ...sheet("Sheet1", [[text("A1"), text("B1")], [text("A2"), text("B2")]]),
        rich: {
          ...defaultRichProjection(),
          cellStyles: { A1: { bold: true }, B1: { italic: true } },
        },
      }],
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
                scope: { type: "columns", start: 0 },
                projection: {
                  ...defaultRichProjection(),
                  cellStyles: { A1: { italic: true } },
                },
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
    expect(result.data?.sheets[0].rich.cellStyles?.B1).toBeUndefined();
  });

  it("shifts column widths locally when column structure metadata omits dimensions", () => {
    const data: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [{
        ...sheet("Sheet1", [[text("A1"), text("B1"), text("C1")]]),
        columnWidths: { 0: 100, 2: 200 },
      }],
    };

    const inserted = applyDocumentPatches(data, [
      {
        type: "ColumnInserted",
        data: {
          patch: {
            sheetIndex: 0,
            colIndex: 1,
            values: [text("inserted")],
            metadata: {
              merges: [],
              rich: { scope: { type: "columns", start: 1 }, projection: defaultRichProjection() },
            },
          },
        },
      },
    ]);

    expect(inserted.data?.sheets[0].columnWidths).toEqual({ 0: 100, 3: 200 });

    const deleted = applyDocumentPatches(inserted.data, [
      {
        type: "ColumnDeleted",
        data: {
          patch: {
            sheetIndex: 0,
            colIndex: 1,
            count: 1,
            metadata: {
              merges: [],
              rich: { scope: { type: "columns", start: 1 }, projection: defaultRichProjection() },
            },
          },
        },
      },
    ]);

    expect(deleted.data?.sheets[0].columnWidths).toEqual({ 0: 100, 2: 200 });
  });

  it("preserves sparse column positions when inserting beyond a row length", () => {
    const data: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("A1")], []])],
    };

    const result = applyDocumentPatches(data, [
      {
        type: "ColumnInserted",
        data: {
          patch: {
            sheetIndex: 0,
            colIndex: 3,
            values: [text("D1"), text("D2")],
            metadata: {
              merges: [],
              rich: { scope: { type: "columns", start: 3 }, projection: defaultRichProjection() },
            },
          },
        },
      },
    ]);

    expect(result.data?.sheets[0].rows).toEqual([
      [text("A1"), blankCell(), blankCell(), text("D1")],
      [blankCell(), blankCell(), blankCell(), text("D2")],
    ]);
  });

  it("fills blank cells when expanding a sheet shape", () => {
    const data: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("A1")]])],
    };

    const result = applyDocumentPatches(data, [
      {
        type: "SheetShape",
        data: {
          patch: {
            sheetIndex: 0,
            rowLengths: [3, 2],
          },
        },
      },
    ]);

    expect(result.data?.sheets[0].rows).toEqual([
      [text("A1"), blankCell(), blankCell()],
      [blankCell(), blankCell()],
    ]);
  });

  it("replaces the sheet tail from a backend restore patch", () => {
    const data: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [
        sheet("Keep", [[text("A")]]),
        sheet("Old 1", [[text("B")]]),
        sheet("Old 2", [[text("C")]]),
      ],
    };
    const result = applyDocumentPatches(data, [
      {
        type: "SheetsReplaced",
        data: {
          patch: {
            startIndex: 1,
            sheets: [sheet("New 1", [[text("D")]]), sheet("New 2", [[text("E")]])],
          },
        },
      },
    ]);

    expect(result.data?.sheets.map((item) => item.name)).toEqual(["Keep", "New 1", "New 2"]);
  });

  it("fails fast when a cell patch targets a missing sheet", () => {
    const data: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("A1")]])],
    };

    expect(() =>
      applyDocumentPatches(data, [
        {
          type: "Cells",
          data: {
            changes: [{ sheetIndex: 1, row: 0, col: 0, value: text("stale") }],
          },
        },
      ])
    ).toThrow("Editor patch targets missing sheet 1");
  });

  it("fails fast when a layout patch targets a missing sheet", () => {
    const data: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("A1")]])],
    };

    expect(() =>
      applyDocumentPatches(data, [
        {
          type: "Layout",
          data: { patch: { sheetIndex: 1, columnWidths: { 0: 120 }, rowHeights: {} } },
        },
      ])
    ).toThrow("Editor patch targets missing sheet 1");
  });

  it("fails fast when a structural patch targets a missing sheet", () => {
    const data: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("A1")]])],
    };

    expect(() =>
      applyDocumentPatches(data, [
        {
          type: "RowDeleted",
          data: {
            patch: {
              sheetIndex: 1,
              rowIndex: 0,
              count: 1,
              metadata: {
                merges: [],
                rich: { scope: { type: "rows", start: 0 }, projection: defaultRichProjection() },
              },
            },
          },
        },
      ])
    ).toThrow("Editor patch targets missing sheet 1");
  });

  it("fails fast when a sheet insert or replace patch has an invalid sheet boundary", () => {
    const data: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("A1")]])],
    };

    expect(() =>
      applyDocumentPatches(data, [
        {
          type: "SheetInserted",
          data: { patch: { sheetIndex: 3, sheet: sheet("Late", [[text("late")]]) } },
        },
      ])
    ).toThrow("Editor patch inserts sheet at invalid index 3");

    expect(() =>
      applyDocumentPatches(data, [
        {
          type: "SheetsReplaced",
          data: { patch: { startIndex: 3, sheets: [sheet("Late", [[text("late")]])] } },
        },
      ])
    ).toThrow("Editor patch replaces sheet tail from invalid index 3");
  });
});
