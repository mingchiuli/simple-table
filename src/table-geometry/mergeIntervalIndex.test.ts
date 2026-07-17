import { describe, expect, it } from "vitest";
import type { MergeRange } from "@/types";
import { MergeIntervalIndex } from "@/table-geometry/mergeIntervalIndex";

describe("MergeIntervalIndex", () => {
  it("stores a full-height merge without expanding it by row", () => {
    const merge = range(0, 2, 249_999, 5);
    const index = new MergeIntervalIndex([merge]);

    expect(index.size).toBe(1);
    expect(index.storedReferenceCount()).toBe(2);
    expect(index.findContaining(200_000, 4)).toBe(merge);
    expect(index.intersecting(249_900, 249_999, 0, 3)).toEqual([merge]);
  });

  it("keeps long-merge storage proportional to merge count", () => {
    const merges = Array.from({ length: 5_000 }, (_, index) =>
      range(index, index % 8, index + 4_095, index % 8 + 1)
    );
    const index = new MergeIntervalIndex(merges);

    expect(index.size).toBe(merges.length);
    expect(index.storedReferenceCount()).toBe(merges.length * 2);
    expect(index.intersecting(4_000, 4_010, 0, 20).length).toBeGreaterThan(0);
  });

  it("matches brute-force point and viewport queries", () => {
    const merges = Array.from({ length: 200 }, (_, index) => {
      const startRow = (index * 37) % 500;
      const startCol = (index * 11) % 40;
      return range(startRow, startCol, startRow + index % 29, startCol + index % 7);
    });
    const index = new MergeIntervalIndex([...merges, merges[0]]);

    for (let row = 0; row < 550; row += 17) {
      for (let col = 0; col < 50; col += 5) {
        const expected = merges.some((merge) => contains(merge, row, col));
        const actual = index.findContaining(row, col);
        expect(actual !== null).toBe(expected);
        if (actual) expect(contains(actual, row, col)).toBe(true);
      }
    }

    const expected = merges
      .filter((merge) => intersects(merge, 120, 260, 8, 22))
      .map(mergeKey)
      .sort();
    const actual = index.intersecting(120, 260, 8, 22).map(mergeKey).sort();
    expect(actual).toEqual(expected);
  });
});

function range(startRow: number, startCol: number, endRow: number, endCol: number): MergeRange {
  return { startRow, startCol, endRow, endCol };
}

function contains(merge: MergeRange, row: number, col: number): boolean {
  return merge.startRow <= row && merge.endRow >= row
    && merge.startCol <= col && merge.endCol >= col;
}

function intersects(
  merge: MergeRange,
  startRow: number,
  endRow: number,
  startCol: number,
  endCol: number
): boolean {
  return merge.startRow <= endRow && merge.endRow >= startRow
    && merge.startCol <= endCol && merge.endCol >= startCol;
}

function mergeKey(merge: MergeRange): string {
  return `${merge.startRow}:${merge.startCol}:${merge.endRow}:${merge.endCol}`;
}
