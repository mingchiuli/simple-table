import type { ComputedRef } from "vue";
import type { MergeRange } from "@/types";

const MAX_INDEXED_MERGE_ROW_SPAN = 4096;

export function useMergeLookup(merges: ComputedRef<MergeRange[]>) {
  const mergeIndex = computed(() => {
    const byRow = new Map<number, MergeRange[]>();
    const largeMerges: MergeRange[] = [];

    for (const merge of merges.value) {
      const rowSpan = merge.endRow - merge.startRow + 1;
      if (rowSpan > MAX_INDEXED_MERGE_ROW_SPAN) {
        largeMerges.push(merge);
        continue;
      }

      for (let row = merge.startRow; row <= merge.endRow; row += 1) {
        const rowMerges = byRow.get(row) ?? [];
        rowMerges.push(merge);
        byRow.set(row, rowMerges);
      }
    }

    for (const rowMerges of byRow.values()) {
      rowMerges.sort((a, b) => a.startCol - b.startCol || a.endCol - b.endCol);
    }

    return { byRow, largeMerges };
  });

  function getMergeInfo(rowIndex: number, colIndex: number): MergeRange | null {
    const indexed = mergeIndex.value.byRow.get(rowIndex);
    const indexedMatch = findMergeInSortedRow(indexed, colIndex);
    if (indexedMatch) return indexedMatch;

    return mergeIndex.value.largeMerges.find((merge) =>
      mergeContains(merge, rowIndex, colIndex)
    ) ?? null;
  }

  function isMergedCell(rowIndex: number, colIndex: number): boolean {
    return getMergeInfo(rowIndex, colIndex) !== null;
  }

  function normalizeCellPosition(rowIndex: number, colIndex: number) {
    const merge = getMergeInfo(rowIndex, colIndex);
    if (!merge) {
      return { rowIndex, colIndex };
    }
    return {
      rowIndex: merge.startRow,
      colIndex: merge.startCol,
    };
  }

  function getMergesIntersecting(
    startRow: number,
    endRow: number,
    startCol: number,
    endCol: number
  ): MergeRange[] {
    const seen = new Set<string>();
    const matches: MergeRange[] = [];
    const { byRow, largeMerges } = mergeIndex.value;

    for (let row = startRow; row <= endRow; row += 1) {
      const rowMerges = byRow.get(row);
      if (!rowMerges) continue;
      for (const merge of rowMerges) {
        if (merge.startCol > endCol) break;
        if (!mergeIntersects(merge, startRow, endRow, startCol, endCol)) continue;
        pushUniqueMerge(matches, seen, merge);
      }
    }

    for (const merge of largeMerges) {
      if (mergeIntersects(merge, startRow, endRow, startCol, endCol)) {
        pushUniqueMerge(matches, seen, merge);
      }
    }

    return matches;
  }

  return {
    getMergeInfo,
    getMergesIntersecting,
    isMergedCell,
    normalizeCellPosition,
  };
}

function findMergeInSortedRow(rowMerges: MergeRange[] | undefined, colIndex: number) {
  if (!rowMerges) return null;
  for (const merge of rowMerges) {
    if (merge.startCol > colIndex) break;
    if (merge.startCol <= colIndex && merge.endCol >= colIndex) {
      return merge;
    }
  }
  return null;
}

function mergeContains(merge: MergeRange, rowIndex: number, colIndex: number): boolean {
  return merge.startRow <= rowIndex
    && merge.endRow >= rowIndex
    && merge.startCol <= colIndex
    && merge.endCol >= colIndex;
}

function mergeIntersects(
  merge: MergeRange,
  startRow: number,
  endRow: number,
  startCol: number,
  endCol: number
): boolean {
  return merge.startRow <= endRow
    && merge.endRow >= startRow
    && merge.startCol <= endCol
    && merge.endCol >= startCol;
}

function pushUniqueMerge(matches: MergeRange[], seen: Set<string>, merge: MergeRange) {
  const key = `${merge.startRow}:${merge.startCol}:${merge.endRow}:${merge.endCol}`;
  if (seen.has(key)) return;
  seen.add(key);
  matches.push(merge);
}
