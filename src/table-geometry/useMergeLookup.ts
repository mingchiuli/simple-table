import type { ComputedRef } from "vue";
import type { MergeRange } from "@/types";

export function useMergeLookup(merges: ComputedRef<MergeRange[]>) {
  function getMergeInfo(rowIndex: number, colIndex: number): MergeRange | null {
    for (const merge of merges.value) {
      if (
        rowIndex >= merge.startRow
        && rowIndex <= merge.endRow
        && colIndex >= merge.startCol
        && colIndex <= merge.endCol
      ) {
        return merge;
      }
    }

    return null;
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
