import { describe, expect, it } from "vitest";
import {
  SparseAxisGeometry,
  collectColumnResizeHandles,
  collectVisibleItems,
} from "@/table-geometry/gridGeometry";

describe("SparseAxisGeometry", () => {
  it("computes offsets from sparse committed and transient sizes", () => {
    const geometry = new SparseAxisGeometry(6, 10, { 1: 20, 4: 5 });

    expect(geometry.offsetAt(0)).toBe(0);
    expect(geometry.offsetAt(2)).toBe(30);
    expect(geometry.totalSize()).toBe(65);
    expect(geometry.sizeAt(4)).toBe(5);

    const transient = { 1: 15, 3: 30 };
    expect(geometry.offsetAt(2, transient)).toBe(25);
    expect(geometry.offsetAt(4, transient)).toBe(65);
    expect(geometry.totalSize(transient)).toBe(80);
  });

  it("finds a deep viewport without materializing all row offsets", () => {
    const geometry = new SparseAxisGeometry(250_000, 72, {
      120_000: 144,
      240_000: 36,
    });
    const scrollTop = 200_000 * 72 + 1 + 72;
    const visible = collectVisibleItems(geometry, scrollTop, 720, 0);

    expect(visible[0].index).toBe(200_000);
    expect(visible.length).toBeLessThan(20);
    expect(visible.at(-1)?.top).toBeLessThanOrEqual(scrollTop + 720);
  });

  it("starts resize handles at the visible deep column", () => {
    const geometry = new SparseAxisGeometry(16_384, 120, { 8_000: 240 });
    const scrollLeft = 10_000 * 120 + 1 + 120;
    const handles = collectColumnResizeHandles(geometry, 60, scrollLeft, 660);

    expect(handles[0].colIndex).toBe(10_000);
    expect(handles.length).toBeLessThan(10);
  });

  it("matches dense reference offsets across sparse overrides", () => {
    const overrides: Record<number, number> = {
      0: 7,
      3: 21,
      9: 4,
      63: 18,
      99: 2,
    };
    const geometry = new SparseAxisGeometry(100, 10, overrides);
    const dense = Array.from({ length: 100 }, (_, index) => overrides[index] ?? 10);

    for (let index = 0; index <= dense.length; index += 1) {
      const expected = dense.slice(0, index).reduce((total, size) => total + size, 0);
      expect(geometry.offsetAt(index)).toBe(expected);
    }
  });
});
