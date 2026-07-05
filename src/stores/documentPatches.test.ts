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

  it("applies row and column structure deltas without replacing the whole sheet", () => {
    const data: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [{
        ...sheet("Sheet1", [[text("A1"), text("B1")], [text("A2"), text("B2")]]),
        merges: [],
        columnWidths: { 1: 140 },
        rowHeights: { 0: 72 },
      }],
    };

    const result = applyDocumentPatches(data, [
      {
        type: "RowsInserted",
        data: { patch: { sheetIndex: 0, rowIndex: 1, rows: [[text("A-new"), text("B-new")]] } },
      },
      {
        type: "ColumnsInserted",
        data: { patch: { sheetIndex: 0, colIndex: 1, values: [text("inserted-1"), text("inserted-new"), text("inserted-2")] } },
      },
    ]);

    expect(result.data?.sheets[0].rows).toEqual([
      [text("A1"), text("inserted-1"), text("B1")],
      [text("A-new"), text("inserted-new"), text("B-new")],
      [text("A2"), text("inserted-2"), text("B2")],
    ]);
    expect(result.data?.sheets[0].merges).toEqual([]);
    expect(result.data?.sheets[0].columnWidths).toEqual({ 2: 140 });
    expect(result.data?.sheets[0].rowHeights).toEqual({ 0: 72 });
    expect(result.data?.sheets[0].rich).toEqual(defaultRichProjection());

    const removed = applyDocumentPatches(result.data, [
      { type: "RowsDeleted", data: { patch: { sheetIndex: 0, rowIndex: 1, count: 1 } } },
      { type: "ColumnsDeleted", data: { patch: { sheetIndex: 0, colIndex: 1, count: 1 } } },
    ]);

    expect(removed.data?.sheets[0].rows).toEqual([
      [text("A1"), text("B1")],
      [text("A2"), text("B2")],
    ]);
  });

  it("keeps row patches self-contained for rich metadata and layout", () => {
    const data: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [{
        ...sheet("Sheet1", [[text("A1")], [text("A2")], [text("A3")]]),
        merges: [{ startRow: 1, startCol: 0, endRow: 2, endCol: 0 }],
        rowHeights: { 1: 72 },
        rich: {
          ...defaultRichProjection(),
          cellStyles: { A2: { bold: true } },
          cellFormats: { A3: { numberFormat: "0.00" } },
          drawings: [{ kind: "image", fromRow: 1, fromCol: 0, toRow: 2, toCol: 0 }],
        },
      }],
    };

    const inserted = applyDocumentPatches(data, [
      { type: "RowsInserted", data: { patch: { sheetIndex: 0, rowIndex: 1, rows: [[text("new")]] } } },
    ]);

    expect(inserted.data?.sheets[0].rowHeights).toEqual({ 2: 72 });
    expect(inserted.data?.sheets[0].rich?.cellStyles?.A3?.bold).toBe(true);
    expect(inserted.data?.sheets[0].rich?.cellFormats?.A4?.numberFormat).toBe("0.00");
    expect(inserted.data?.sheets[0].rich?.drawings?.[0]).toMatchObject({ fromRow: 2, toRow: 3 });
    expect(inserted.data?.sheets[0].merges).toEqual([
      { startRow: 2, startCol: 0, endRow: 3, endCol: 0 },
    ]);

    const deleted = applyDocumentPatches(inserted.data, [
      { type: "RowsDeleted", data: { patch: { sheetIndex: 0, rowIndex: 1, count: 1 } } },
    ]);

    expect(deleted.data?.sheets[0].rowHeights).toEqual({ 1: 72 });
    expect(deleted.data?.sheets[0].rich?.cellStyles?.A2?.bold).toBe(true);
    expect(deleted.data?.sheets[0].rich?.cellFormats?.A3?.numberFormat).toBe("0.00");
    expect(deleted.data?.sheets[0].rich?.drawings?.[0]).toMatchObject({ fromRow: 1, toRow: 2 });
    expect(deleted.data?.sheets[0].merges).toEqual([
      { startRow: 1, startCol: 0, endRow: 2, endCol: 0 },
    ]);
  });

  it("keeps column patches self-contained for rich metadata and layout", () => {
    const data: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [{
        ...sheet("Sheet1", [[text("A1"), text("B1"), text("C1")]]),
        merges: [{ startRow: 0, startCol: 1, endRow: 0, endCol: 2 }],
        columnWidths: { 1: 144 },
        rich: {
          ...defaultRichProjection(),
          cellStyles: { B1: { italic: true } },
          cellFormats: { C1: { numberFormat: "0%" } },
          drawings: [{ kind: "image", fromRow: 0, fromCol: 1, toRow: 0, toCol: 2 }],
        },
      }],
    };

    const inserted = applyDocumentPatches(data, [
      { type: "ColumnsInserted", data: { patch: { sheetIndex: 0, colIndex: 1, values: [text("new")] } } },
    ]);

    expect(inserted.data?.sheets[0].columnWidths).toEqual({ 2: 144 });
    expect(inserted.data?.sheets[0].rich?.cellStyles?.C1?.italic).toBe(true);
    expect(inserted.data?.sheets[0].rich?.cellFormats?.D1?.numberFormat).toBe("0%");
    expect(inserted.data?.sheets[0].rich?.drawings?.[0]).toMatchObject({ fromCol: 2, toCol: 3 });
    expect(inserted.data?.sheets[0].merges).toEqual([
      { startRow: 0, startCol: 2, endRow: 0, endCol: 3 },
    ]);

    const deleted = applyDocumentPatches(inserted.data, [
      { type: "ColumnsDeleted", data: { patch: { sheetIndex: 0, colIndex: 1, count: 1 } } },
    ]);

    expect(deleted.data?.sheets[0].columnWidths).toEqual({ 1: 144 });
    expect(deleted.data?.sheets[0].rich?.cellStyles?.B1?.italic).toBe(true);
    expect(deleted.data?.sheets[0].rich?.cellFormats?.C1?.numberFormat).toBe("0%");
    expect(deleted.data?.sheets[0].rich?.drawings?.[0]).toMatchObject({ fromCol: 1, toCol: 2 });
    expect(deleted.data?.sheets[0].merges).toEqual([
      { startRow: 0, startCol: 1, endRow: 0, endCol: 2 },
    ]);
  });
});
