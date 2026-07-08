import { describe, expect, it } from "vitest";
import { applyRichProjectionPatch } from "@/stores/richProjectionPatch";
import { defaultRichProjection } from "@/types";

describe("applyRichProjectionPatch", () => {
  it("merges row scoped rich metadata without touching rows before the scope", () => {
    const result = applyRichProjectionPatch(
      {
        ...defaultRichProjection(),
        cellStyles: {
          A1: { bold: true },
          A3: { italic: true },
        },
        hiddenRows: [0, 3],
        hiddenColumns: [2],
        drawings: [
          { kind: "image", fromRow: 0, fromCol: 0, toRow: 0, toCol: 1 },
          { kind: "image", fromRow: 3, fromCol: 0, toRow: 4, toCol: 1 },
        ],
      },
      {
        scope: { type: "rows", start: 2 },
        projection: {
          ...defaultRichProjection(),
          cellStyles: { A3: { backgroundColor: "#fff" } },
          hiddenRows: [2],
          drawings: [{ kind: "image", fromRow: 2, fromCol: 0, toRow: 3, toCol: 1 }],
        },
      }
    );

    expect(result.cellStyles?.A1).toEqual({ bold: true });
    expect(result.cellStyles?.A3).toEqual({ backgroundColor: "#fff" });
    expect(result.hiddenRows).toEqual([0, 2]);
    expect(result.hiddenColumns).toEqual([2]);
    expect(result.drawings).toEqual([
      { kind: "image", fromRow: 0, fromCol: 0, toRow: 0, toCol: 1 },
      { kind: "image", fromRow: 2, fromCol: 0, toRow: 3, toCol: 1 },
    ]);
  });

  it("merges column scoped rich metadata without touching columns before the scope", () => {
    const result = applyRichProjectionPatch(
      {
        ...defaultRichProjection(),
        cellFormats: {
          A1: { numberFormat: "0" },
          C1: { numberFormat: "0.00" },
        },
        hiddenRows: [1],
        hiddenColumns: [0, 2],
        freezePane: {
          topLeftCell: "C3",
          horizontalSplit: 2,
          verticalSplit: 2,
          activePane: "bottomRight",
          state: "frozen",
        },
      },
      {
        scope: { type: "columns", start: 1 },
        projection: {
          ...defaultRichProjection(),
          cellFormats: { B1: { numberFormat: "@" } },
          hiddenColumns: [1],
          freezePane: null,
        },
      }
    );

    expect(result.cellFormats?.A1).toEqual({ numberFormat: "0" });
    expect(result.cellFormats?.B1).toEqual({ numberFormat: "@" });
    expect(result.cellFormats?.C1).toBeUndefined();
    expect(result.hiddenRows).toEqual([1]);
    expect(result.hiddenColumns).toEqual([0, 1]);
    expect(result.freezePane).toBeNull();
  });
});
