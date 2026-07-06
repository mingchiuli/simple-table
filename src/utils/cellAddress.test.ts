import { describe, expect, it } from "vitest";
import { cellKey, deleteRichRows, parseCellKey, shiftRichColumns } from "@/utils/cellAddress";
import { defaultRichProjection } from "@/types";

describe("cellAddress", () => {
  it("round-trips Excel-style cell keys", () => {
    expect(cellKey(0, 0)).toBe("A1");
    expect(cellKey(9, 26)).toBe("AA10");
    expect(parseCellKey("AA10")).toEqual({ row: 9, col: 26 });
  });

  it("moves rich projection maps with structured addresses", () => {
    const shifted = shiftRichColumns({
      ...defaultRichProjection(),
      cellStyles: { A1: { bold: true }, B1: { italic: true } },
      hiddenColumns: [0, 1],
      freezePane: {
        topLeftCell: "B2",
        horizontalSplit: 1,
        verticalSplit: 1,
        activePane: "BottomRight",
        state: "Frozen",
      },
      hyperlinks: { B1: { url: "https://example.com", location: false } },
      drawings: [{ kind: "image", fromRow: 0, fromCol: 1, toRow: 1, toCol: 2 }],
    }, 1, 1);

    expect(shifted.cellStyles?.A1?.bold).toBe(true);
    expect(shifted.cellStyles?.C1?.italic).toBe(true);
    expect(shifted.hiddenColumns).toEqual([0, 2]);
    expect(shifted.freezePane?.topLeftCell).toBe("C2");
    expect(shifted.hyperlinks?.C1?.url).toBe("https://example.com");
    expect(shifted.drawings?.[0]).toMatchObject({ fromCol: 2, toCol: 3 });
  });

  it("drops deleted rich projection cells", () => {
    const shifted = deleteRichRows({
      ...defaultRichProjection(),
      cellFormats: { A1: { numberFormat: "0" }, A2: { numberFormat: "0.00" } },
      hiddenRows: [0, 1, 3],
      freezePane: {
        topLeftCell: "A2",
        horizontalSplit: 1,
        verticalSplit: 1,
        activePane: "BottomRight",
        state: "Frozen",
      },
      hyperlinks: {
        A1: { url: "https://deleted.example", location: false },
        A2: { url: "https://kept.example", location: false },
      },
    }, 0, 1);

    expect(shifted.cellFormats?.A1?.numberFormat).toBe("0.00");
    expect(Object.keys(shifted.cellFormats ?? {})).toEqual(["A1"]);
    expect(shifted.hiddenRows).toEqual([0, 2]);
    expect(shifted.freezePane?.topLeftCell).toBe("A1");
    expect(shifted.hyperlinks?.A1?.url).toBe("https://kept.example");
  });
});
