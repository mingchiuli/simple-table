import type { MergeRange } from "@/types";

type MergeIntervalNode = {
  center: number;
  byStart: MergeRange[];
  byEnd: MergeRange[];
  left: MergeIntervalNode | null;
  right: MergeIntervalNode | null;
};

export class MergeIntervalIndex {
  private readonly root: MergeIntervalNode | null;
  readonly size: number;

  constructor(merges: readonly MergeRange[]) {
    const unique = uniqueValidMerges(merges);
    this.root = buildNode(unique);
    this.size = unique.length;
  }

  findContaining(row: number, col: number): MergeRange | null {
    return findContaining(this.root, row, col);
  }

  intersecting(
    startRow: number,
    endRow: number,
    startCol: number,
    endCol: number
  ): MergeRange[] {
    if (startRow > endRow || startCol > endCol) return [];
    const matches: MergeRange[] = [];
    collectIntersecting(
      this.root,
      startRow,
      endRow,
      startCol,
      endCol,
      matches
    );
    return matches;
  }

  storedReferenceCount(): number {
    return countStoredReferences(this.root);
  }
}

function buildNode(merges: MergeRange[]): MergeIntervalNode | null {
  if (merges.length === 0) return null;
  const midpoints = merges
    .map((merge) => merge.startRow + Math.floor((merge.endRow - merge.startRow) / 2))
    .sort((left, right) => left - right);
  const center = midpoints[Math.floor(midpoints.length / 2)];
  const left: MergeRange[] = [];
  const right: MergeRange[] = [];
  const overlapping: MergeRange[] = [];

  for (const merge of merges) {
    if (merge.endRow < center) left.push(merge);
    else if (merge.startRow > center) right.push(merge);
    else overlapping.push(merge);
  }

  return {
    center,
    byStart: overlapping.sort(compareByStart),
    byEnd: [...overlapping].sort(compareByEndDescending),
    left: buildNode(left),
    right: buildNode(right),
  };
}

function findContaining(
  node: MergeIntervalNode | null,
  row: number,
  col: number
): MergeRange | null {
  if (!node) return null;
  if (row < node.center) {
    for (const merge of node.byStart) {
      if (merge.startRow > row) break;
      if (containsColumn(merge, col)) return merge;
    }
    return findContaining(node.left, row, col);
  }
  if (row > node.center) {
    for (const merge of node.byEnd) {
      if (merge.endRow < row) break;
      if (containsColumn(merge, col)) return merge;
    }
    return findContaining(node.right, row, col);
  }
  return node.byStart.find((merge) => containsColumn(merge, col)) ?? null;
}

function collectIntersecting(
  node: MergeIntervalNode | null,
  startRow: number,
  endRow: number,
  startCol: number,
  endCol: number,
  matches: MergeRange[]
) {
  if (!node) return;

  if (endRow < node.center) {
    for (const merge of node.byStart) {
      if (merge.startRow > endRow) break;
      if (columnsIntersect(merge, startCol, endCol)) matches.push(merge);
    }
    collectIntersecting(node.left, startRow, endRow, startCol, endCol, matches);
    return;
  }

  if (startRow > node.center) {
    for (const merge of node.byEnd) {
      if (merge.endRow < startRow) break;
      if (columnsIntersect(merge, startCol, endCol)) matches.push(merge);
    }
    collectIntersecting(node.right, startRow, endRow, startCol, endCol, matches);
    return;
  }

  for (const merge of node.byStart) {
    if (columnsIntersect(merge, startCol, endCol)) matches.push(merge);
  }
  collectIntersecting(node.left, startRow, endRow, startCol, endCol, matches);
  collectIntersecting(node.right, startRow, endRow, startCol, endCol, matches);
}

function uniqueValidMerges(merges: readonly MergeRange[]): MergeRange[] {
  const unique = new Map<string, MergeRange>();
  for (const merge of merges) {
    if (!isValidMerge(merge)) continue;
    const key = `${merge.startRow}:${merge.startCol}:${merge.endRow}:${merge.endCol}`;
    unique.set(key, merge);
  }
  return [...unique.values()];
}

function isValidMerge(merge: MergeRange): boolean {
  return Number.isInteger(merge.startRow)
    && Number.isInteger(merge.endRow)
    && Number.isInteger(merge.startCol)
    && Number.isInteger(merge.endCol)
    && merge.startRow >= 0
    && merge.startCol >= 0
    && merge.endRow >= merge.startRow
    && merge.endCol >= merge.startCol;
}

function containsColumn(merge: MergeRange, col: number): boolean {
  return merge.startCol <= col && merge.endCol >= col;
}

function columnsIntersect(merge: MergeRange, startCol: number, endCol: number): boolean {
  return merge.startCol <= endCol && merge.endCol >= startCol;
}

function compareByStart(left: MergeRange, right: MergeRange): number {
  return left.startRow - right.startRow
    || left.startCol - right.startCol
    || left.endRow - right.endRow
    || left.endCol - right.endCol;
}

function compareByEndDescending(left: MergeRange, right: MergeRange): number {
  return right.endRow - left.endRow
    || left.startCol - right.startCol
    || left.startRow - right.startRow
    || left.endCol - right.endCol;
}

function countStoredReferences(node: MergeIntervalNode | null): number {
  if (!node) return 0;
  return node.byStart.length
    + node.byEnd.length
    + countStoredReferences(node.left)
    + countStoredReferences(node.right);
}
