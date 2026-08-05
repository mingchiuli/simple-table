import type { ComputedRef, Ref } from "vue";
import type { CellFormatProjection, CellStyleProjection, CellValue } from "@/types";
import type { SheetViewportModel } from "@/table-geometry/sheetViewportModel";
import {
  areNumberRecordsEqual,
  collectColumnResizeHandles,
  collectRowResizeHandles,
  collectVisibleItems,
  SparseAxisGeometry,
  type GridItem,
} from "@/table-geometry/gridGeometry";
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
  format?: CellFormatProjection;
  style?: CellStyleProjection;
};

export type MergeOverlayCell = CellItem & {
  draftValue?: string;
  selected: boolean;
};

type UseGridGeometryOptions = {
  sheet: ComputedRef<SheetViewportModel>;
  selectedCell: ComputedRef<{ row: number; col: number } | null | undefined>;
  tableSize: Ref<{ width: number; height: number }>;
  scrollLeft: Ref<number>;
  scrollTop: Ref<number>;
  rowHeaderWidth: number;
  headerHeight: number;
  defaultColumnWidth: number;
  defaultRowHeight: number;
  minimumColumnWidth: number;
  minimumRowHeight: number;
  overscanPx: number;
  getDraftValue: (rowIndex: number, colIndex: number) => string | undefined;
};

export function useGridGeometry({
  sheet,
  selectedCell,
  tableSize,
  scrollLeft,
  scrollTop,
  rowHeaderWidth,
  headerHeight,
  defaultColumnWidth,
  defaultRowHeight,
  minimumColumnWidth,
  minimumRowHeight,
  overscanPx,
  getDraftValue,
}: UseGridGeometryOptions) {
  const committedColumnWidths = ref<Record<number, number>>({});
  const committedRowHeights = ref<Record<number, number>>({});
  const previewColumnWidths = ref<Record<number, number>>({});
  const previewRowHeights = ref<Record<number, number>>({});
  const merges = computed(() => sheet.value.merges);
  const sourceColumnWidths = computed(() => sheet.value.columnWidths);
  const sourceRowHeights = computed(() => sheet.value.rowHeights);
  const { getMergesIntersecting, isMergedCell, normalizeCellPosition } = useMergeLookup(merges);

  function syncColumnWidths() {
    const nextWidths = { ...sourceColumnWidths.value };
    if (!areNumberRecordsEqual(committedColumnWidths.value, nextWidths)) {
      committedColumnWidths.value = nextWidths;
    }
    previewColumnWidths.value = {};
  }

  function syncRowHeights() {
    const nextHeights = { ...sourceRowHeights.value };
    if (!areNumberRecordsEqual(committedRowHeights.value, nextHeights)) {
      committedRowHeights.value = nextHeights;
    }
    previewRowHeights.value = {};
  }

  syncColumnWidths();
  syncRowHeights();

  watch(sourceColumnWidths, syncColumnWidths, { deep: true });
  watch(sourceRowHeights, syncRowHeights, { deep: true });

  const viewportWidth = computed(() => Math.max(0, tableSize.value.width - rowHeaderWidth));
  const viewportHeight = computed(() => Math.max(0, tableSize.value.height - headerHeight));
  const sheetExtent = computed(() => sheet.value.extent);

  const columnCount = computed(() => sheetExtent.value.columnCount);
  const rowCount = computed(() => sheetExtent.value.rowCount);

  const columnGeometry = computed(() => new SparseAxisGeometry(
    columnCount.value,
    defaultColumnWidth,
    committedColumnWidths.value,
    minimumColumnWidth
  ));
  const rowGeometry = computed(() => new SparseAxisGeometry(
    rowCount.value,
    defaultRowHeight,
    committedRowHeights.value,
    minimumRowHeight
  ));
  const totalColumnsWidth = computed(() =>
    columnGeometry.value.totalSize(previewColumnWidths.value)
  );
  const totalRowsHeight = computed(() => rowGeometry.value.totalSize(previewRowHeights.value));

  const visibleRows = computed<GridItem[]>(() =>
    collectVisibleItems(
      rowGeometry.value,
      scrollTop.value,
      viewportHeight.value,
      overscanPx,
      previewRowHeights.value
    )
  );

  const visibleColumns = computed<ColumnItem[]>(() =>
    collectVisibleItems(
      columnGeometry.value,
      scrollLeft.value,
      viewportWidth.value,
      overscanPx,
      previewColumnWidths.value
    ).map((item) => ({
      index: item.index,
      title: sheet.value.columnTitleAt(item.index),
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
          value: sheet.value.cellAt(row.index, column.index),
          format: cellFormat(row.index, column.index),
          style: cellStyle(row.index, column.index),
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

    const firstRow = visibleRows.value[0]?.index;
    const lastRow = visibleRows.value.at(-1)?.index;
    const firstCol = visibleColumns.value[0]?.index;
    const lastCol = visibleColumns.value.at(-1)?.index;
    if (
      firstRow === undefined
      || lastRow === undefined
      || firstCol === undefined
      || lastCol === undefined
    ) {
      return [];
    }

    return getMergesIntersecting(firstRow, lastRow, firstCol, lastCol).flatMap((merge) => {
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
        value: sheet.value.cellAt(merge.startRow, merge.startCol),
        format: cellFormat(merge.startRow, merge.startCol),
        style: cellStyle(merge.startRow, merge.startCol),
        draftValue: getDraftValue(merge.startRow, merge.startCol),
        selected: selectedCell.value?.row === merge.startRow && selectedCell.value?.col === merge.startCol,
      }];
    });
  });

  const visibleColumnResizeHandles = computed(() =>
    collectColumnResizeHandles(
      columnGeometry.value,
      rowHeaderWidth,
      scrollLeft.value,
      tableSize.value.width,
      previewColumnWidths.value
    )
  );

  const visibleRowResizeHandles = computed(() =>
    collectRowResizeHandles(
      rowGeometry.value,
      headerHeight,
      scrollTop.value,
      tableSize.value.height,
      previewRowHeights.value
    )
  );

  function getColumnWidth(colIndex: number): number {
    return columnGeometry.value.sizeAt(colIndex, previewColumnWidths.value);
  }

  function getRowHeight(rowIndex: number): number {
    return rowGeometry.value.sizeAt(rowIndex, previewRowHeights.value);
  }

  function getRowOffset(rowIndex: number): number {
    return rowGeometry.value.offsetAt(rowIndex, previewRowHeights.value);
  }

  function getDataColumnOffset(colIndex: number): number {
    return columnGeometry.value.offsetAt(colIndex, previewColumnWidths.value);
  }

  function getColumnOffset(colIndex: number): number {
    return rowHeaderWidth + getDataColumnOffset(colIndex);
  }

  function getDataColumnIndexAt(left: number): number {
    return columnGeometry.value.indexAt(Math.max(0, left) + 0.001, previewColumnWidths.value);
  }

  function getRowIndexAt(top: number): number {
    return rowGeometry.value.indexAt(Math.max(0, top) + 0.001, previewRowHeights.value);
  }

  function getColumnSpanWidth(startCol: number, endCol: number): number {
    return Math.max(0, getDataColumnOffset(endCol + 1) - getDataColumnOffset(startCol));
  }

  function getRowSpanHeight(startRow: number, endRow: number): number {
    return Math.max(0, getRowOffset(endRow + 1) - getRowOffset(startRow));
  }

  function setColumnWidth(colIndex: number, width: number) {
    previewColumnWidths.value = {
      ...previewColumnWidths.value,
      [colIndex]: width,
    };
  }

  function setRowHeight(rowIndex: number, height: number) {
    previewRowHeights.value = {
      ...previewRowHeights.value,
      [rowIndex]: height,
    };
  }

  function clearColumnWidth(colIndex: number) {
    const next = { ...previewColumnWidths.value };
    delete next[colIndex];
    previewColumnWidths.value = next;
  }

  function clearRowHeight(rowIndex: number) {
    const next = { ...previewRowHeights.value };
    delete next[rowIndex];
    previewRowHeights.value = next;
  }

  function cellFormat(rowIndex: number, colIndex: number): CellFormatProjection | undefined {
    return sheet.value.formatAt(rowIndex, colIndex);
  }

  function cellStyle(rowIndex: number, colIndex: number): CellStyleProjection | undefined {
    return sheet.value.styleAt(rowIndex, colIndex);
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
    getDataColumnIndexAt,
    getRowIndexAt,
    setColumnWidth,
    setRowHeight,
    clearColumnWidth,
    clearRowHeight,
    normalizeCellPosition,
  };
}
