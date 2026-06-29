import type { ComputedRef, Ref } from "vue";
import type { CellValue, MergeRange } from "@/types";
import {
  areNumberRecordsEqual,
  buildOffsets,
  collectColumnResizeHandles,
  collectRowResizeHandles,
  collectVisibleItems,
  offsetAt,
  spanSize,
  type GridItem,
} from "@/table-geometry/gridGeometry";
import { calculateSheetExtent } from "@/table-geometry/sheetExtent";
import { useMergeLookup } from "@/table-geometry/useMergeLookup";

export type ColumnItem = {
  index: number;
  title: string;
  left: number;
  width: number;
};

export type CellItem = {
  key: string;
  rowIndex: number;
  colIndex: number;
  top: number;
  left: number;
  width: number;
  height: number;
  value: CellValue | undefined;
};

export type MergeOverlayCell = CellItem & {
  draftValue?: string;
  selected: boolean;
};

type UseGridGeometryOptions = {
  data: ComputedRef<CellValue[][]>;
  columns: ComputedRef<string[]>;
  merges: ComputedRef<MergeRange[]>;
  selectedCell: ComputedRef<{ row: number; col: number } | null | undefined>;
  columnWidths: ComputedRef<Record<number, number> | undefined>;
  rowHeights: ComputedRef<Record<number, number> | undefined>;
  tableSize: Ref<{ width: number; height: number }>;
  scrollLeft: Ref<number>;
  scrollTop: Ref<number>;
  rowHeaderWidth: number;
  headerHeight: number;
  defaultColumnWidth: number;
  defaultRowHeight: number;
  overscanPx: number;
  getDraftValue: (rowIndex: number, colIndex: number) => string | undefined;
};

export function useGridGeometry({
  data,
  columns,
  merges,
  selectedCell,
  columnWidths: sourceColumnWidths,
  rowHeights: sourceRowHeights,
  tableSize,
  scrollLeft,
  scrollTop,
  rowHeaderWidth,
  headerHeight,
  defaultColumnWidth,
  defaultRowHeight,
  overscanPx,
  getDraftValue,
}: UseGridGeometryOptions) {
  const columnWidths = ref<Record<number, number>>({});
  const rowHeights = ref<Record<number, number>>({});
  const { isMergedCell, normalizeCellPosition } = useMergeLookup(merges);

  function syncColumnWidths() {
    const nextWidths = sourceColumnWidths.value ? { ...sourceColumnWidths.value } : {};
    if (areNumberRecordsEqual(columnWidths.value, nextWidths)) return;
    columnWidths.value = nextWidths;
  }

  function syncRowHeights() {
    const nextHeights = sourceRowHeights.value ? { ...sourceRowHeights.value } : {};
    if (areNumberRecordsEqual(rowHeights.value, nextHeights)) return;
    rowHeights.value = nextHeights;
  }

  syncColumnWidths();
  syncRowHeights();

  watch(sourceColumnWidths, syncColumnWidths, { deep: true });
  watch(sourceRowHeights, syncRowHeights, { deep: true });

  const viewportWidth = computed(() => Math.max(0, tableSize.value.width - rowHeaderWidth));
  const viewportHeight = computed(() => Math.max(0, tableSize.value.height - headerHeight));
  const sheetExtent = computed(() =>
    calculateSheetExtent(data.value, merges.value, columnWidths.value, rowHeights.value)
  );

  const columnCount = computed(() => Math.max(columns.value.length, sheetExtent.value.columnCount));
  const rowCount = computed(() => sheetExtent.value.rowCount);

  const columnOffsets = computed(() => buildOffsets(columnCount.value, getColumnWidth));
  const rowOffsets = computed(() => buildOffsets(rowCount.value, getRowHeight));
  const totalColumnsWidth = computed(() => columnOffsets.value.at(-1) ?? 0);
  const totalRowsHeight = computed(() => rowOffsets.value.at(-1) ?? 0);

  const visibleRows = computed<GridItem[]>(() =>
    collectVisibleItems(rowOffsets.value, rowCount.value, scrollTop.value, viewportHeight.value, overscanPx)
  );

  const visibleColumns = computed<ColumnItem[]>(() =>
    collectVisibleItems(
      columnOffsets.value,
      columnCount.value,
      scrollLeft.value,
      viewportWidth.value,
      overscanPx
    ).map((item) => ({
      index: item.index,
      title: columns.value[item.index] ?? "",
      left: item.top,
      width: item.height,
    }))
  );

  const visibleCellItems = computed<CellItem[]>(() => {
    const cells: CellItem[] = [];
    for (const row of visibleRows.value) {
      for (const column of visibleColumns.value) {
        if (isMergedCell(row.index, column.index)) continue;
        cells.push({
          key: `${row.index}-${column.index}`,
          rowIndex: row.index,
          colIndex: column.index,
          top: row.top,
          left: column.left,
          width: column.width,
          height: row.height,
          value: data.value[row.index]?.[column.index],
        });
      }
    }
    return cells;
  });

  const visibleMergeCells = computed<MergeOverlayCell[]>(() => {
    const leftLimit = scrollLeft.value - overscanPx;
    const rightLimit = scrollLeft.value + viewportWidth.value + overscanPx;
    const topLimit = scrollTop.value - overscanPx;
    const bottomLimit = scrollTop.value + viewportHeight.value + overscanPx;

    return merges.value.flatMap((merge) => {
      const left = getDataColumnOffset(merge.startCol);
      const top = getRowOffset(merge.startRow);
      const width = getColumnSpanWidth(merge.startCol, merge.endCol);
      const height = getRowSpanHeight(merge.startRow, merge.endRow);

      if (
        width <= 0
        || height <= 0
        || left + width < leftLimit
        || left > rightLimit
        || top + height < topLimit
        || top > bottomLimit
      ) {
        return [];
      }

      return [{
        key: `${merge.startRow}-${merge.startCol}-${merge.endRow}-${merge.endCol}`,
        rowIndex: merge.startRow,
        colIndex: merge.startCol,
        top,
        left,
        width,
        height,
        value: data.value[merge.startRow]?.[merge.startCol],
        draftValue: getDraftValue(merge.startRow, merge.startCol),
        selected: selectedCell.value?.row === merge.startRow && selectedCell.value?.col === merge.startCol,
      }];
    });
  });

  const visibleColumnResizeHandles = computed(() =>
    collectColumnResizeHandles(
      columnCount.value,
      rowHeaderWidth,
      scrollLeft.value,
      tableSize.value.width,
      getColumnWidth
    )
  );

  const visibleRowResizeHandles = computed(() =>
    collectRowResizeHandles(
      rowCount.value,
      headerHeight,
      scrollTop.value,
      tableSize.value.height,
      getRowHeight
    )
  );

  function getColumnWidth(colIndex: number): number {
    return columnWidths.value[colIndex] || defaultColumnWidth;
  }

  function getRowHeight(rowIndex: number): number {
    return rowHeights.value[rowIndex] || defaultRowHeight;
  }

  function getRowOffset(rowIndex: number): number {
    return offsetAt(rowOffsets.value, rowIndex, totalRowsHeight.value);
  }

  function getDataColumnOffset(colIndex: number): number {
    return offsetAt(columnOffsets.value, colIndex, totalColumnsWidth.value);
  }

  function getColumnOffset(colIndex: number): number {
    return rowHeaderWidth + getDataColumnOffset(colIndex);
  }

  function getColumnSpanWidth(startCol: number, endCol: number): number {
    return spanSize(columnOffsets.value, startCol, endCol, totalColumnsWidth.value);
  }

  function getRowSpanHeight(startRow: number, endRow: number): number {
    return spanSize(rowOffsets.value, startRow, endRow, totalRowsHeight.value);
  }

  function setColumnWidth(colIndex: number, width: number) {
    columnWidths.value = {
      ...columnWidths.value,
      [colIndex]: width,
    };
  }

  function setRowHeight(rowIndex: number, height: number) {
    rowHeights.value = {
      ...rowHeights.value,
      [rowIndex]: height,
    };
  }

  return {
    viewportWidth,
    viewportHeight,
    totalColumnsWidth,
    totalRowsHeight,
    visibleRows,
    visibleColumns,
    visibleCellItems,
    visibleMergeCells,
    visibleColumnResizeHandles,
    visibleRowResizeHandles,
    getColumnWidth,
    getRowHeight,
    getRowOffset,
    getColumnOffset,
    getDataColumnOffset,
    setColumnWidth,
    setRowHeight,
    normalizeCellPosition,
  };
}
