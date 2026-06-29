import type { ComputedRef } from "vue";
import type { MergeRange } from "@/types";

export function useMergeLookup(merges: ComputedRef<MergeRange[]>) {
  const mergeIndex = computed(() => {
    const covered = new Map<string, MergeRange>();
    for (const merge of merges.value) {
      for (let row = merge.startRow; row <= merge.endRow; row += 1) {
        for (let col = merge.startCol; col <= merge.endCol; col += 1) {
          covered.set(cellKey(row, col), merge);
        }
      }
    }
    return covered;
  });

  function getMergeInfo(rowIndex: number, colIndex: number): MergeRange | null {
    return mergeIndex.value.get(cellKey(rowIndex, colIndex)) ?? null;
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

  return {
    getMergeInfo,
    isMergedCell,
    normalizeCellPosition,
  };
}

function cellKey(rowIndex: number, colIndex: number): string {
  return `${rowIndex}:${colIndex}`;
}
