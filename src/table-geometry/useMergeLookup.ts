import type { ComputedRef } from "vue";
import type { MergeRange } from "@/types";
import { MergeIntervalIndex } from "@/table-geometry/mergeIntervalIndex";

export function useMergeLookup(merges: ComputedRef<MergeRange[]>) {
  const mergeIndex = computed(() => new MergeIntervalIndex(merges.value));

  function getMergeInfo(rowIndex: number, colIndex: number): MergeRange | null {
    return mergeIndex.value.findContaining(rowIndex, colIndex);
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
    return mergeIndex.value.intersecting(startRow, endRow, startCol, endCol);
  }

  return {
    getMergeInfo,
    getMergesIntersecting,
    isMergedCell,
    normalizeCellPosition,
  };
}
