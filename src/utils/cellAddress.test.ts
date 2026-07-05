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
      drawings: [{ kind: "image", fromRow: 0, fromCol: 1, toRow: 1, toCol: 2 }],
    }, 1, 1);

    expect(shifted.cellStyles?.A1?.bold).toBe(true);
    expect(shifted.cellStyles?.C1?.italic).toBe(true);
    expect(shifted.drawings?.[0]).toMatchObject({ fromCol: 2, toCol: 3 });
  });

  it("drops deleted rich projection cells", () => {
    const shifted = deleteRichRows({
      ...defaultRichProjection(),
      cellFormats: { A1: { numberFormat: "0" }, A2: { numberFormat: "0.00" } },
    }, 0, 1);

    expect(shifted.cellFormats?.A1?.numberFormat).toBe("0.00");
    expect(Object.keys(shifted.cellFormats ?? {})).toEqual(["A1"]);
  });
});
