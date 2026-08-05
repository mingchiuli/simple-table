import { describe, expect, it } from "vitest";
import { defaultRichProjection } from "@/types";
import { calculateSheetExtent } from "@/table-geometry/sheetExtent";

describe("calculateSheetExtent", () => {
  it("includes rich projection anchors in the editable extent", () => {
    const extent = calculateSheetExtent(
      [],
      [],
      undefined,
      undefined,
      {
        ...defaultRichProjection(),
        cellStyles: { C4: { bold: true } },
        hyperlinks: { B5: { url: "https://example.com", location: false } },
        hiddenRows: [8],
        hiddenColumns: [6],
        drawings: [{ kind: "chart", fromRow: 10, fromCol: 7, toRow: 11, toCol: 8 }],
      }
    );

    expect(extent).toEqual({ rowCount: 12, columnCount: 9 });
  });
});
