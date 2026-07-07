import { describe, expect, it } from "vitest";
import { cellKey, parseCellKey } from "@/utils/cellAddress";

describe("cellAddress", () => {
  it("round-trips Excel-style cell keys", () => {
    expect(cellKey(0, 0)).toBe("A1");
    expect(cellKey(9, 26)).toBe("AA10");
    expect(parseCellKey("AA10")).toEqual({ row: 9, col: 26 });
  });
});
